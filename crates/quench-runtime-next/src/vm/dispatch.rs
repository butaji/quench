use super::*;

impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn step(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: Instr,
        pc: &mut usize,
    ) -> Result<Option<Value>, JsError> {
        match i.op() {
            Op::Nop => {}
            Op::LoadConst => self.write(f, i.a(), self.constants[i.imm() as usize]),
            Op::LoadLocal => {
                // SAFETY: compiler construction and residual decoding establish
                // the frame-local bound before interpretation.
                let v = unsafe {
                    *self
                        .frames
                        .get_unchecked(f)
                        .locals
                        .get_unchecked(i.imm() as usize)
                };
                self.write(f, i.a(), v);
            }
            Op::StoreLocal => {
                let value = self.read(f, i.a());
                self.frames[f].locals[i.imm() as usize] = value;
                if i.b() != 0 {
                    self.write(f, i.b() - 1, value);
                }
            }
            Op::LoadEnvLocal => {
                let value = if self.frames[f].captured {
                    let Some(Cell::Environment { slots, .. }) = self.heap.get(self.frames[f].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    slots[i.imm() as usize]
                } else {
                    self.frames[f].locals[i.imm() as usize]
                };
                self.write(f, i.a(), value);
            }
            Op::StoreEnvLocal => {
                let value = self.read(f, i.a());
                if self.frames[f].captured {
                    let Some(Cell::Environment { slots, .. }) =
                        self.heap.get_mut(self.frames[f].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    slots[i.imm() as usize] = value;
                } else {
                    self.frames[f].locals[i.imm() as usize] = value;
                }
            }
            Op::LoadCapture => {
                let v = self.capture(f, i.imm())?;
                self.write(f, i.a(), v);
            }
            Op::StoreCapture => self.store_capture(f, i.imm(), self.read(f, i.a()))?,
            Op::LoadName => {
                let v = self.load_name(p, i.imm(), i.c())?;
                self.write(f, i.a(), v);
            }
            Op::StoreName => self.store_name(p, i.imm(), self.read(f, i.a()), i.c())?,
            Op::LoadThis => self.write(f, i.a(), self.frames[f].this),
            Op::MakeClosure => {
                let env = self.promote_frame_environment(f);
                let v = self.closure(p, i.imm(), env)?;
                self.write(f, i.a(), v);
            }
            Op::MakeObject => {
                let v = self.object();
                self.write(f, i.a(), v);
            }
            Op::MakeObject2 => {
                let v = self.object_pair(
                    p,
                    i.imm() as usize,
                    self.read(f, i.b()),
                    self.read(f, i.c()),
                );
                if i.a() & RETURN_REGISTER != 0 {
                    return Ok(Some(v));
                }
                self.write(f, i.a() & REGISTER_MASK, v);
            }
            Op::SuperConstArrayObject2 => {
                if let Some(value) = self.execute_const_array_object2(p, f, i.a(), i.imm())? {
                    return Ok(Some(value));
                }
            }
            Op::MakeArray => {
                let v = self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![Value::UNDEFINED; i.imm() as usize]),
                });
                self.write(f, i.a(), v);
            }
            Op::MakeConstArray => {
                let start = i.imm() as usize;
                let end = start + i.b() as usize;
                let elements = self.const_arrays[start]
                    .get_or_insert_with(|| Rc::new(self.constants[start..end].to_vec()))
                    .clone();
                let v = self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements,
                });
                self.write(f, i.a(), v);
            }
            Op::GetField => {
                let v = if i.b() == FieldBase::NESTED {
                    self.resolve_field(p, f, i.imm())?
                } else {
                    let base = self.resolve_field_base(f, FieldBase(i.b()));
                    self.get_field_cached(p, base, i.imm(), i.c())?
                };
                if i.a() & SET_THIS_REGISTER != 0 {
                    let sink = p.field_sites[i.imm() as usize]
                        .sink
                        .expect("fused field sink");
                    self.set_field_cached(self.frames[f].this, sink.0, v, sink.1)?;
                }
                if i.a() & RETURN_REGISTER != 0 {
                    return Ok(Some(v));
                }
                self.write(f, i.a() & REGISTER_MASK, v);
            }
            Op::GetIndex => {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_dispatch(false, false);
                let v = self.get_index(p, self.read(f, i.b()), self.read(f, i.c()))?;
                self.write(f, i.a(), v);
            }
            Op::SetField => {
                self.set_field_cached(self.read(f, i.b()), i.imm(), self.read(f, i.a()), i.c())?
            }
            Op::SetThisField => {
                self.set_field_cached(self.frames[f].this, i.imm(), self.read(f, i.a()), i.c())?
            }
            Op::SetIndex => {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_dispatch(true, false);
                self.set_index(
                    p,
                    self.read(f, i.b()),
                    self.read(f, i.c()),
                    self.read(f, i.a()),
                )?
            }
            Op::Move => self.write(f, i.a(), self.read(f, i.b())),
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                self.profile.binary(i.imm() as usize, i.b(), i.c());
                let left = self.resolve_operand(p, f, Operand(i.b()))?;
                let right = self.resolve_operand(p, f, Operand(i.c()))?;
                #[cfg(feature = "profile-aggregate")]
                self.profile_regional_binary(f, *pc - 1, i.imm(), left, right);
                let v = self.binary(p, i.imm(), left, right)?;
                if i.a() & RETURN_REGISTER != 0 {
                    return Ok(Some(v));
                }
                self.write(f, i.a() & REGISTER_MASK, v);
            }
            Op::IncDec => {
                let input = self.read(f, i.b());
                let delta = if i.imm() == 0 { 1.0 } else { -1.0 };
                let value = if let Some(integer) = input.as_int() {
                    let next = if i.imm() == 0 {
                        integer.checked_add(1)
                    } else {
                        integer.checked_sub(1)
                    };
                    next.map(Value::integer)
                        .unwrap_or_else(|| Value::number(f64::from(integer) + delta))
                } else {
                    Value::number(self.to_number(p, input)? + delta)
                };
                self.write(f, i.a(), value);
            }
            Op::Unary => {
                let v = self.unary(p, i.imm(), self.read(f, i.b()))?;
                self.write(f, i.a(), v);
            }
            Op::Jump => {
                *pc = i.imm() as usize;
                self.frames[f].pc = *pc;
                self.maybe_collect(p);
            }
            Op::JumpFalse => {
                let value = self.read(f, i.a());
                let truthy = self.truthy(value);
                #[cfg(feature = "profile-aggregate")]
                self.profile.branch_value(value.profile_kind(), truthy);
                if !truthy {
                    *pc = i.imm() as usize;
                }
            }
            Op::JumpBinaryFalse => {
                self.profile.binary(i.a() as usize, i.b(), i.c());
                let left = self.resolve_operand(p, f, Operand(i.b()))?;
                let right = self.resolve_operand(p, f, Operand(i.c()))?;
                if !self.binary_truthy(p, u32::from(i.a()), left, right)? {
                    *pc = i.imm() as usize;
                }
            }
            Op::Call => {
                self.profile.call_source(0);
                let base = (i.imm() >> 16) as u16;
                let n = i.imm() as u16;
                let mut inline = [std::mem::MaybeUninit::<Value>::uninit(); 8];
                debug_assert!(n <= 8, "wide calls need an out-of-line argument buffer");
                for x in 0..n {
                    inline[x as usize].write(self.read(f, base + x));
                }
                let this = self.read(f, i.c());
                let this = if this.is_undefined() {
                    self.globals
                } else {
                    this
                };
                let callee = self.read(f, i.b());
                // SAFETY: the compiler caps calls at eight arguments and this
                // loop initialized exactly the prefix exposed to the callee.
                let args = unsafe {
                    std::slice::from_raw_parts(inline.as_ptr().cast::<Value>(), n as usize)
                };
                self.frames[f].pc = *pc;
                let value = self.call_value(p, callee, this, args)?;
                if i.a() & RETURN_REGISTER != 0 {
                    self.profile.terminal_call(0);
                    return Ok(Some(value));
                }
                self.write(f, i.a(), value);
            }
            Op::CallKnown => {
                self.profile.call_source(1);
                let base = (i.imm() >> 16) as u16;
                let n = i.imm() as u16;
                let mut inline = [std::mem::MaybeUninit::<Value>::uninit(); 8];
                for x in 0..n {
                    inline[x as usize].write(self.read(f, base + x));
                }
                // SAFETY: the compiler caps calls at eight initialized arguments.
                let args = unsafe {
                    std::slice::from_raw_parts(inline.as_ptr().cast::<Value>(), n as usize)
                };
                let parent = self.capture_env(f, 0).unwrap_or(self.frames[f].env);
                self.profile.call_target(1, n as usize);
                self.frames[f].pc = *pc;
                let value = self.call_user(p, u32::from(i.b()), parent, self.globals, args)?;
                if i.a() & RETURN_REGISTER != 0 {
                    self.profile.terminal_call(0);
                    return Ok(Some(value));
                }
                self.write(f, i.a(), value);
            }
            Op::CallMethod => {
                self.profile.call_source(2);
                let this = self.read(f, i.b());
                self.frames[f].pc = *pc;
                let value = self.call_method_site(p, f, i.imm() as usize, this)?;
                if i.a() & RETURN_REGISTER != 0 {
                    self.profile.terminal_call(1);
                    return Ok(Some(value));
                }
                self.write(f, i.a(), value);
            }
            Op::CallThisMethod => {
                self.profile.call_source(3);
                let path = p.method_sites[i.imm() as usize].receiver_path;
                let this = if let Some((atom, cache)) = path {
                    self.get_field_cached(p, self.frames[f].this, atom, cache)?
                } else {
                    self.frames[f].this
                };
                self.frames[f].pc = *pc;
                let value = self.call_method_site(p, f, i.imm() as usize, this)?;
                if i.a() & RETURN_REGISTER != 0 {
                    self.profile.terminal_call(2);
                    return Ok(Some(value));
                }
                self.write(f, i.a(), value);
            }
            Op::Construct => {
                self.profile.call_source(4);
                let n = i.imm() as u16;
                let mut inline = [std::mem::MaybeUninit::<Value>::uninit(); 8];
                debug_assert!(
                    n <= 8,
                    "wide constructors need an out-of-line argument buffer"
                );
                for x in 0..n {
                    inline[x as usize].write(self.read(f, i.c() + x));
                }
                // SAFETY: identical initialized-prefix invariant to `Call`.
                let args = unsafe {
                    std::slice::from_raw_parts(inline.as_ptr().cast::<Value>(), n as usize)
                };
                self.frames[f].pc = *pc;
                let v = self.construct_value(p, self.read(f, i.b()), args)?;
                if i.a() & RETURN_REGISTER != 0 {
                    return Ok(Some(v));
                }
                self.write(f, i.a(), v);
            }
            Op::Return => return Ok(Some(self.read(f, i.a()))),
            Op::Throw => return Err(JsError(self.to_string(p, self.read(f, i.a()))?.into())),
        }
        Ok(None)
    }
    #[inline(always)]
    pub(super) fn resolve_operand(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        operand: Operand,
    ) -> Result<Value, JsError> {
        let tag = operand.tag();
        self.profile.operand(tag as usize);
        match tag {
            0 => Ok(self.read(frame, operand.payload() as Register)),
            1 => Ok(self.constants[operand.payload() as usize]),
            2 => self.resolve_field(p, frame, u32::from(operand.payload())),
            3 => {
                let slot = operand.payload() as usize;
                if self.frames[frame].captured {
                    let Some(Cell::Environment { slots, .. }) =
                        self.heap.get(self.frames[frame].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    Ok(slots[slot])
                } else {
                    Ok(self.frames[frame].locals[slot])
                }
            }
            _ => unreachable!("two-bit operand tag"),
        }
    }
    #[inline(always)]
    pub(super) fn resolve_field(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site: u32,
    ) -> Result<Value, JsError> {
        let site = p.field_sites[site as usize];
        let base = self.resolve_field_base(frame, site.base);
        let value = self.get_field_cached(p, base, site.first.0, site.first.1)?;
        match site.second {
            Some((atom, cache)) => self.get_field_cached(p, value, atom, cache),
            None => Ok(value),
        }
    }
}
