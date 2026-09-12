macro_rules! define_guest_frame_abi {
    (
        $visibility:vis struct $name:ident {
            slot_value: $slot_value:ty,
            result_value: $result_value:ty,
            site: $site:ty,
            region_array: $region_array:ty,
            callback_owner: $callback_owner:ty,
            name_ic: $name_ic:ty,
            environment_access: $environment_access:ty $(,)?
        }
    ) => {
        type RegionGuardEntry =
            unsafe extern "C" fn(*mut $callback_owner, usize) -> usize;
        type DirectCallEntry =
            unsafe extern "C" fn(*mut $callback_owner, *const $site) -> usize;
        type InstanceOfConditionEntry =
            unsafe extern "C" fn(*mut $callback_owner, *const $site) -> usize;

        const REGISTER_VALUES_FRAME_WORD_OFFSET: usize = 0;
        const LOCAL_VALUES_FRAME_WORD_OFFSET: usize = REGISTER_VALUES_FRAME_WORD_OFFSET + 1;
        const LOCAL_COUNT_FRAME_WORD_OFFSET: usize = LOCAL_VALUES_FRAME_WORD_OFFSET + 1;
        const CURRENT_SITE_FRAME_WORD_OFFSET: usize = LOCAL_COUNT_FRAME_WORD_OFFSET + 1;
        const SITES_FRAME_WORD_OFFSET: usize = CURRENT_SITE_FRAME_WORD_OFFSET + 1;
        const NAME_SNAPSHOTS_FRAME_WORD_OFFSET: usize = SITES_FRAME_WORD_OFFSET + 1;
        const RESULT_FRAME_WORD_OFFSET: usize = NAME_SNAPSHOTS_FRAME_WORD_OFFSET + 1;
        const REGION_ARRAYS_FRAME_WORD_OFFSET: usize = RESULT_FRAME_WORD_OFFSET + 1;
        const REGION_GUARD_FRAME_WORD_OFFSET: usize = REGION_ARRAYS_FRAME_WORD_OFFSET + 1;
        const REGION_ITERATIONS_FRAME_WORD_OFFSET: usize = REGION_GUARD_FRAME_WORD_OFFSET + 1;
        const DIRECT_CALL_FRAME_WORD_OFFSET: usize = REGION_ITERATIONS_FRAME_WORD_OFFSET + 1;
        const RETURN_TARGET_FRAME_WORD_OFFSET: usize = DIRECT_CALL_FRAME_WORD_OFFSET + 1;
        const RESUME_TARGET_FRAME_WORD_OFFSET: usize = RETURN_TARGET_FRAME_WORD_OFFSET + 1;
        const INSTANCEOF_CONDITION_FRAME_WORD_OFFSET: usize = RESUME_TARGET_FRAME_WORD_OFFSET + 1;
        const ENVIRONMENT_ACCESS_CHAIN_FRAME_WORD_OFFSET: usize =
            INSTANCEOF_CONDITION_FRAME_WORD_OFFSET + 1;
        const ENVIRONMENT_ACCESS_CHAIN_LEN_FRAME_WORD_OFFSET: usize =
            ENVIRONMENT_ACCESS_CHAIN_FRAME_WORD_OFFSET + 1;
        const NAME_ICS_FRAME_WORD_OFFSET: usize =
            ENVIRONMENT_ACCESS_CHAIN_LEN_FRAME_WORD_OFFSET + 1;
        const GUEST_FRAME_HEADER_WORDS: usize = NAME_ICS_FRAME_WORD_OFFSET + 1;
        const NAME_IC_WORDS: usize = 3;
        const ENVIRONMENT_ACCESS_WORDS: usize = 3;

        #[derive(Clone, Copy)]
        #[repr(C)]
        $visibility struct $name {
            register_values: *mut $slot_value,
            local_values: *mut $slot_value,
            local_count: usize,
            current_site: *const $site,
            sites: *const $site,
            name_snapshots: *const $slot_value,
            result: $result_value,
            region_arrays: *const $region_array,
            region_guard: RegionGuardEntry,
            region_iterations: u64,
            direct_call: DirectCallEntry,
            return_target: usize,
            resume_target: usize,
            instanceof_condition: InstanceOfConditionEntry,
            environment_access_chain: *const *const $environment_access,
            environment_access_chain_len: usize,
            name_ics: *const $name_ic,
        }

        const _: () = assert!(core::mem::size_of::<usize>() == core::mem::size_of::<u64>());
        const _: () = assert!(core::mem::size_of::<$slot_value>() == core::mem::size_of::<usize>());
        const _: () = assert!(core::mem::align_of::<$slot_value>() == core::mem::align_of::<usize>());
        const _: () = assert!(core::mem::size_of::<$result_value>() == core::mem::size_of::<usize>());
        const _: () = assert!(core::mem::align_of::<$result_value>() == core::mem::align_of::<usize>());
        const _: () = assert!(core::mem::align_of::<$name>() == core::mem::align_of::<usize>());
        const _: () = assert!(
            core::mem::size_of::<$name_ic>()
                == NAME_IC_WORDS * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::size_of::<$environment_access>()
                == ENVIRONMENT_ACCESS_WORDS * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::size_of::<$name>()
                == GUEST_FRAME_HEADER_WORDS * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, register_values)
                == REGISTER_VALUES_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, local_values)
                == LOCAL_VALUES_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, local_count)
                == LOCAL_COUNT_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, current_site)
                == CURRENT_SITE_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, sites)
                == SITES_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, name_snapshots)
                == NAME_SNAPSHOTS_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, result)
                == RESULT_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, region_arrays)
                == REGION_ARRAYS_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, region_guard)
                == REGION_GUARD_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, region_iterations)
                == REGION_ITERATIONS_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, direct_call)
                == DIRECT_CALL_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, return_target)
                == RETURN_TARGET_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, resume_target)
                == RESUME_TARGET_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, instanceof_condition)
                == INSTANCEOF_CONDITION_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, environment_access_chain)
                == ENVIRONMENT_ACCESS_CHAIN_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, environment_access_chain_len)
                == ENVIRONMENT_ACCESS_CHAIN_LEN_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
        const _: () = assert!(
            core::mem::offset_of!($name, name_ics)
                == NAME_ICS_FRAME_WORD_OFFSET * core::mem::size_of::<usize>()
        );
    };
}
