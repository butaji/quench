//! RegExp literal sites: the object each evaluation makes, or the one object a receiver-only
//! or callee-argument site shares while it cannot escape.
use super::*;

impl<H: Host> Vm<H> {
    pub(in crate::vm) fn regexp_literal(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site_index: usize,
    ) -> Result<Value, JsError> {
        self.regexp_literal_of(p, self.frames[frame].program, site_index)
    }

    /// The new RegExp object regular expression literal site `site_index` of `p`, the
    /// residual of `program`, evaluates to.
    pub(in crate::vm) fn regexp_literal_of(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        site_index: usize,
    ) -> Result<Value, JsError> {
        let site = p.regexp_literal_sites.get(site_index)
            .ok_or_else(|| JsError::validation("RegExp literal site is outside program".into()))?;
        if site.receiver {
            return self.regexp_literal_receiver(p, program, site_index);
        }
        match self.regexp_from_cached_site(program, site_index) {
            Some(object) => Ok(object),
            None => self.regexp_literal_fresh(p, program, site_index),
        }
    }

    /// The new object of literal site `site_index` made from the site's pattern and flags,
    /// whose record the site keeps for its later evaluations.
    pub(in crate::vm) fn regexp_literal_fresh(&mut self, p: &ResidualProgram, program: ProgramId, site_index: usize) -> Result<Value, JsError> {
        let site = p.regexp_literal_sites.get(site_index)
            .ok_or_else(|| JsError::validation("RegExp literal site is outside program".into()))?;
        let source = self.programs.constant(program, site.pattern_constant as usize)
            .and_then(|value| self.heap.string(value).cloned())
            .ok_or_else(|| JsError::validation("RegExp literal pattern is not a string".into()))?;
        let flags_text = self.programs.constant(program, site.flags_constant as usize)
            .filter(|value| self.heap.string(*value).is_some())
            .ok_or_else(|| JsError::validation("RegExp literal flags are not a string".into()))?;
        let (prototype, constructor) = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .map(|intrinsics| (intrinsics.prototype, intrinsics.constructor))
            .expect("RegExp intrinsics are installed for the active realm");
        let owner = crate::heap::RegExpLegacyOwner::Enabled(constructor);
        let meta = match self.programs.regexp_literal_meta(program, site_index) {
            Some(meta) => meta,
            None => self.regexp_literal_site_meta(program, site_index, (source, flags_text), owner)?,
        };
        // A site's record names the realm it was first evaluated in; another realm's
        // evaluation owns a record of its own.
        let meta = if meta.legacy_constructor.constructor() == constructor {
            meta
        } else {
            Rc::new(crate::heap::RegExpMeta { legacy_constructor: owner, ..(*meta).clone() })
        };
        self.regexp_from_meta(prototype, meta)
    }

    /// The record of literal site `site_index` of `program`, made from its `source` and flags
    /// text and kept for the site's later evaluations.
    fn regexp_literal_site_meta(
        &mut self,
        program: ProgramId,
        site_index: usize,
        (source, flags_text): (JsString, Value),
        owner: crate::heap::RegExpLegacyOwner,
    ) -> Result<Rc<crate::heap::RegExpMeta>, JsError> {
        let flags = self.heap.string(flags_text).expect("checked above").host_string();
        // The compiler checked the literal's pattern and flags (early errors), so its
        // matcher compiles on the first match (`Vm::regexp_matcher`), as a snapshot's
        // patterns do: a module evaluates every literal it holds, and most never run
        // (tr46's 34 Unicode classes were 3.3% of the whatwg-url npm scenario).
        let meta = Rc::new(crate::heap::RegExpMeta {
            source,
            flags: RegExpFlags::validated(flags),
            legacy_constructor: owner,
            matcher: crate::heap::RegExpMatcher::default(),
        });
        if !self.programs.cache_regexp_literal_meta(program, site_index, Rc::clone(&meta)) {
            return Err(JsError::validation("RegExp literal cache site is invalid".into()));
        }
        Ok(meta)
    }

    /// The object of receiver-only literal site `site_index` (`RegExpLiteralSite::receiver`):
    /// one per site while `RegExp.prototype.test` and `exec` are the intrinsics and never
    /// changed (`Fuses::ever_changed`), so only the builtins ever see it, and its flags hold
    /// neither `g` nor `y`, so they never write its `lastIndex`. Each evaluation is then a
    /// fresh object in every observable way; any other evaluation makes a fresh one.
    fn regexp_literal_receiver(&mut self, p: &ResidualProgram, program: ProgramId, site_index: usize) -> Result<Value, JsError> {
        let reusable = self.regexp_builtins_never_replaced();
        if reusable && let Some(object) = self.programs.regexp_literal_receiver(program, site_index) {
            return Ok(object);
        }
        let object = match self.regexp_from_cached_site(program, site_index) {
            Some(object) => object,
            None => self.regexp_literal_fresh(p, program, site_index)?,
        };
        let stateless = matches!(self.heap.get(object), Some(Cell::RegExp { meta, .. })
            if !meta.flags.contains(RegExpFlags::GLOBAL) && !meta.flags.contains(RegExpFlags::STICKY));
        if reusable && stateless {
            let fuses = self.heap.dependencies_mut().fuses_mut();
            fuses.guard(crate::code_dependencies::IntactFuse::RegExpTest);
            fuses.guard(crate::code_dependencies::IntactFuse::RegExpExec);
            self.programs.cache_regexp_literal_receiver(program, site_index, object);
        }
        Ok(object)
    }

    /// The object callee-argument literal site `site_index` of `p`, the residual of
    /// `program`, evaluates to in frame `frame`, where `callee` is the callee of the call the
    /// literal is the first argument of (`FunctionCompiler::regexp_argument_call`).
    ///
    /// Invariant: each evaluation of a literal makes a new object, so the site may answer
    /// one shared object only where no evaluation can tell it from a fresh one. That holds
    /// while the object has never reached guest code and holds a fresh object's state:
    /// - `callee` is this realm's `String.prototype.replace`, `match`, `search` or `split`,
    ///   which hands its first argument to nothing but `GetMethod(regexp, @@symbol)` and the
    ///   call of that method (`string_replace_native` and its siblings);
    /// - `RegExp.prototype`'s `@@replace`, `@@match`, `@@search` and `@@split` are this
    ///   realm's builtins, its `exec`, `flags` and flag getters are the builtins and no own
    ///   property shadows them (`verify_regexp_builtin_layout`), and its `constructor` is the
    ///   realm's RegExp, whose `@@species` is the intrinsic getter (`IntactFuse::RegExpSpecies`):
    ///   each builtin then reads the object only through builtins and passes it to no guest
    ///   function, and `@@split`'s splitter is a new RegExp. Proved when the object is made,
    ///   and kept by a watch on the prototype's values and by the fuse: any write that could
    ///   let a later use hand the object to guest code retires them, after which the site
    ///   makes a new object (the old one, which a use may have handed out, is never answered
    ///   again);
    /// - its `lastIndex` holds 0, as a fresh object's does: a use that completes leaves it 0
    ///   (a global `@@replace`/`@@match` ends at 0, `@@search` restores it, `@@split` and a
    ///   non-global, non-sticky exec never write it), and any other value sends the site to
    ///   a new object;
    /// - no two uses overlap: it is answered again only in the frame that made it. The
    ///   compiler admits only later arguments that cannot suspend the frame, so between the
    ///   literal and the call the frame stays on the stack, and any guest code a use runs
    ///   (a replacer, a `toString`) runs in frames above it, where the site makes its own.
    ///
    /// The builtins run on the object as on a fresh one, so the legacy statics (`RegExp.$1`)
    /// are recorded as for any other.
    ///
    /// Every tier answers the site here: `frame` is the evaluating frame's index in the
    /// interpreter, a frame's lane run and the b-jit (whose helper step is `Vm::step`), and in
    /// the Cranelift tiers the payload's frame (`lane_allocations::regexp_argument_key`),
    /// which no live frame shares.
    pub(in crate::vm) fn regexp_literal_argument_of(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        frame: usize,
        site_index: usize,
        callee: Value,
    ) -> Result<Value, JsError> {
        let callee_intact = self.regexp_argument_callee(callee);
        if callee_intact && let Some(object) = self.regexp_argument_share_hit(program, frame, site_index) {
            return Ok(object);
        }
        let object = match self.regexp_from_cached_site(program, site_index) {
            Some(object) => object,
            None => self.regexp_literal_fresh(p, program, site_index)?,
        };
        if !callee_intact {
            // The object goes to an unknown callee; the shared one, if any, stays unexposed.
            return Ok(object);
        }
        match self.regexp_argument_share_proof(object) {
            Some((constructor, watch)) => {
                let realm = self.realm.globals;
                let share = crate::vm::program_store::RegExpArgumentShare { object, frame, realm, constructor, watch };
                self.programs.cache_regexp_literal_argument(program, site_index, share);
            }
            None => self.programs.forget_regexp_literal_argument(program, site_index),
        }
        Ok(object)
    }

    /// Whether `callee` is this realm's `String.prototype.replace`, `match`, `search` or
    /// `split`.
    fn regexp_argument_callee(&self, callee: Value) -> bool {
        matches!(
            self.heap.function(callee),
            Some(record @ FunctionRecord { kind: FunctionKind::Native(native), .. })
                if self.record_realm(record) == self.realm.globals
                    && matches!(native, Native::StringReplace | Native::StringMatch | Native::StringSearch | Native::StringSplit)
        )
    }

    /// The object literal site `site_index` of `program` shares, where every condition of
    /// `regexp_literal_argument_of` still holds for an evaluation in `frame`.
    fn regexp_argument_share_hit(&mut self, program: ProgramId, frame: usize, site_index: usize) -> Option<Value> {
        let (object, held_frame, realm, constructor, published) = self
            .programs
            .regexp_literal_argument(program, site_index)
            .map(|share| (share.object, share.frame, share.realm, share.constructor, share.watch.is_published()))?;
        if held_frame != frame || realm != self.realm.globals || !published {
            return None;
        }
        let fuses = self.heap.dependencies().fuses();
        if fuses.intact(crate::code_dependencies::IntactFuse::RegExpSpecies, constructor) != Some(true) {
            return None;
        }
        let (_, last_index) = self.regexp_last_index_value(object)?;
        (last_index.as_number() == Some(0.0)).then_some(object)
    }

    /// The realm's RegExp constructor and a watch on the values of `RegExp.prototype`, where
    /// the fresh literal `object` meets the builtin conditions of `regexp_literal_argument_of`:
    /// its prototype's `@@replace`, `@@match`, `@@search` and `@@split` are this realm's
    /// builtins, its `exec`, `flags` and flag getters the builtins, its `constructor` the
    /// realm's RegExp, whose `@@species` is the intrinsic getter. The watch is taken before
    /// the values are read, so any later write to them retires it.
    fn regexp_argument_share_proof(&mut self, object: Value) -> Option<(Value, crate::heap::PrototypeValueWatch)> {
        use crate::code_dependencies::IntactFuse;
        let constructor = self.regexp_intrinsic_constructor();
        // Proving the fuse may make the constructor a watched prototype, which changes a
        // layout and so retires a watch taken before it.
        if !self.fuse_intact_at(IntactFuse::RegExpSpecies, constructor) {
            return None;
        }
        let prototype = self.object_data(object)?.proto();
        self.heap.mark_prototype(prototype);
        let watch = crate::heap::watch_prototype_values(self.heap.object(prototype)?.exact_shape())?;
        self.verify_regexp_builtin_layout(object, prototype)?;
        for (symbol, native) in [
            (WellKnownSymbol::Replace, Native::RegExpSymbolReplace),
            (WellKnownSymbol::Match, Native::RegExpSymbolMatch),
            (WellKnownSymbol::Search, Native::RegExpSymbolSearch),
            (WellKnownSymbol::Split, Native::RegExpSymbolSplit),
        ] {
            let symbol = self.well_known_symbols.get(symbol)?;
            let slot = self.verify_regexp_symbol_layout(object, prototype, symbol)?;
            let method = self.heap.property_get(self.object_data(prototype)?, slot)?;
            if !self.is_current_realm_native(method, native) {
                return None;
            }
        }
        let constructor_atom = self.intern_atom("constructor");
        let constructor_key = PropertyKey::string(constructor_atom);
        let constructor_held = self.property_attributes(object, constructor_key).is_none()
            && self.property_attributes(prototype, constructor_key).is_some_and(|attributes| !attributes.accessor)
            && self.own_property(prototype, constructor_atom) == Some(constructor);
        let last_index_zero = self
            .regexp_last_index_value(object)
            .is_some_and(|(_, value)| value.as_number() == Some(0.0));
        (constructor_held && last_index_zero && watch.is_published() && self.fuse_intact_at(IntactFuse::RegExpSpecies, constructor))
            .then_some((constructor, watch))
    }

    /// Whether `RegExp.prototype.test` and `exec` are this realm's intrinsics and their
    /// holder never changed them since they were proved.
    fn regexp_builtins_never_replaced(&mut self) -> bool {
        use crate::code_dependencies::IntactFuse;
        let fuses = [IntactFuse::RegExpTest, IntactFuse::RegExpExec];
        fuses.into_iter().all(|fuse| self.fuse_intact(fuse)) && !fuses.into_iter().any(|fuse| self.heap.dependencies().fuses().ever_changed(fuse))
    }
}
