use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn profile_regional_binary(
        &mut self,
        frame: usize,
        pc: usize,
        operator: u32,
        left: Value,
        right: Value,
    ) -> bool {
        let fast = self.numeric_binary(operator, left, right).is_some();
        #[cfg(feature = "profile-aggregate")]
        self.profile
            .regional_binary(self.frames[frame].function, pc as u32, fast);
        let key = (self.frames[frame].function, pc as u32);
        let site = self.numeric_sites.entry(key).or_default();
        if !self.specialized {
            site.armed = false;
            return false;
        }
        if fast {
            if site.slow_path == 0 {
                site.consistent_fast = site.consistent_fast.saturating_add(1);
                if site.consistent_fast >= 8 {
                    site.armed = true;
                }
            } else {
                site.consistent_fast = 1;
                site.slow_path = 0;
                site.armed = false;
            }
        } else {
            site.consistent_fast = 0;
            site.slow_path = site.slow_path.saturating_add(1);
            site.armed = false;
        }
        site.armed
    }

    #[inline]
    pub(super) fn deopt_numeric_site(&mut self, frame: usize, pc: usize) {
        let key = (self.frames[frame].function, pc as u32);
        if let Some(site) = self.numeric_sites.get_mut(&key) {
            site.consistent_fast = 0;
            site.slow_path = site.slow_path.saturating_add(1);
            site.armed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SilentHost;

    impl Host for SilentHost {
        fn write_line(&mut self, _: &str) {}

        fn clock_millis(&mut self) -> f64 {
            0.0
        }
    }

    fn vm() -> Vm<SilentHost> {
        let mut vm = Vm::new(SilentHost);
        vm.frames.push(Frame {
            function: 3,
            pc: 0,
            env: Value::NULL,
            this: Value::UNDEFINED,
            locals: vec![],
            captured: false,
            registers: vec![],
        });
        vm
    }

    #[test]
    fn numeric_feedback_arms_after_consistent_fast_observations() {
        let mut vm = vm();
        for _ in 0..7 {
            assert!(!vm.profile_regional_binary(0, 4, 8, Value::integer(1), Value::integer(2)));
        }
        assert!(vm.profile_regional_binary(0, 4, 8, Value::integer(1), Value::integer(2)));
    }

    #[test]
    fn numeric_feedback_deoptimizes_on_type_miss() {
        let mut vm = vm();
        for _ in 0..8 {
            vm.profile_regional_binary(0, 4, 8, Value::integer(1), Value::integer(2));
        }
        assert!(vm.numeric_sites[&(3, 4)].armed);
        assert!(!vm.profile_regional_binary(0, 4, 8, Value::number(1.5), Value::integer(2)));
        assert!(!vm.numeric_sites[&(3, 4)].armed);
    }

    #[test]
    fn generic_programs_never_arm_numeric_sites() {
        let mut vm = vm();
        vm.specialized = false;
        for _ in 0..16 {
            assert!(!vm.profile_regional_binary(0, 4, 8, Value::integer(1), Value::integer(2)));
        }
        assert!(!vm.numeric_sites[&(3, 4)].armed);
    }
}
