use crate::bytecode::{Function, Op};

pub(super) fn report(functions: &[Function]) {
    let mut captured = functions
        .iter()
        .map(|function| vec![false; function.locals as usize])
        .collect::<Vec<_>>();
    let mut defining_closures = vec![0usize; functions.len()];
    let mut capture_loads = 0;
    let mut capture_stores = 0;
    let mut environment_loads = 0;
    let mut environment_stores = 0;
    let mut max_depth = 0;
    for (id, function) in functions.iter().enumerate() {
        for instruction in &function.code {
            match instruction.op() {
                Op::MakeClosure => defining_closures[id] += 1,
                Op::LoadEnvLocal => environment_loads += 1,
                Op::StoreEnvLocal => environment_stores += 1,
                Op::LoadCapture | Op::StoreCapture => {
                    capture_loads += usize::from(instruction.op() == Op::LoadCapture);
                    capture_stores += usize::from(instruction.op() == Op::StoreCapture);
                    let depth = usize::from(instruction.capture_depth());
                    max_depth = max_depth.max(depth);
                    if let Some(owner) = ancestor(functions, id, depth + 1) {
                        let slot = usize::from(instruction.capture_slot());
                        if slot < captured[owner].len() {
                            captured[owner][slot] = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let promoted_slots = defining_closures
        .iter()
        .enumerate()
        .filter(|(_, closures)| **closures != 0)
        .map(|(id, _)| functions[id].locals as usize)
        .sum::<usize>();
    let captured_slots = captured.iter().flatten().filter(|slot| **slot).count();
    let closures_without_direct_free_variables = functions
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, function)| {
            !function
                .code
                .iter()
                .any(|instruction| matches!(instruction.op(), Op::LoadCapture | Op::StoreCapture))
        })
        .count();
    eprintln!(
        "{{\"kind\":\"rqj-capture-census\",\"functions\":{},\"closure_sites\":{},\"defining_functions\":{},\"promoted_slots\":{promoted_slots},\"captured_slots\":{captured_slots},\"avoidable_slots\":{},\"promoted_bytes\":{},\"captured_bytes\":{},\"environment_loads\":{environment_loads},\"environment_stores\":{environment_stores},\"capture_loads\":{capture_loads},\"capture_stores\":{capture_stores},\"max_capture_depth\":{max_depth},\"closures_without_direct_free_variables\":{closures_without_direct_free_variables}}}",
        functions.len(),
        defining_closures.iter().sum::<usize>(),
        defining_closures
            .iter()
            .filter(|count| **count != 0)
            .count(),
        promoted_slots.saturating_sub(captured_slots),
        promoted_slots * size_of::<crate::value::Value>(),
        captured_slots * size_of::<crate::value::Value>(),
    );
}

fn ancestor(functions: &[Function], mut function: usize, distance: usize) -> Option<usize> {
    for _ in 0..distance {
        function = functions.get(function)?.parent? as usize;
    }
    Some(function)
}
