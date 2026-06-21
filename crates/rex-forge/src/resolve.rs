//! Pure dependency/conflict solver. No I/O.
use crate::error::ResolveError;
use crate::registry::Registry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvePlan {
    pub base: String,
    pub components: Vec<String>,
}

/// Resolve a requested component set against a base:
/// 1. every requested component must exist and declare `base` in its `bases`,
/// 2. `requires` are pulled in transitively,
/// 3. no two components in the final set may conflict.
///
/// The final component list is sorted and deduped (deterministic).
pub fn resolve(
    reg: &Registry,
    base: &str,
    requested: &[String],
) -> Result<ResolvePlan, ResolveError> {
    let mut final_set: Vec<String> = Vec::new();
    let mut queue: Vec<String> = requested.to_vec();

    while let Some(name) = queue.pop() {
        if final_set.contains(&name) {
            continue;
        }
        let comp = reg
            .component(&name)
            .ok_or_else(|| ResolveError::UnknownComponent(name.clone()))?;
        if !comp.bases.iter().any(|b| b == base) {
            return Err(ResolveError::BaseMismatch {
                component: name.clone(),
                base: base.to_string(),
            });
        }
        for req in &comp.requires {
            if !final_set.contains(req) {
                queue.push(req.clone());
            }
        }
        final_set.push(name);
    }

    // Conflict check over the final set (symmetric).
    for i in 0..final_set.len() {
        for j in (i + 1)..final_set.len() {
            let (a, b) = (&final_set[i], &final_set[j]);
            let a_conf = reg.component(a).map(|c| c.conflicts.contains(b)).unwrap_or(false);
            let b_conf = reg.component(b).map(|c| c.conflicts.contains(a)).unwrap_or(false);
            if a_conf || b_conf {
                let (mut x, mut y) = (a.clone(), b.clone());
                if x > y {
                    std::mem::swap(&mut x, &mut y);
                }
                return Err(ResolveError::Conflict { a: x, b: y });
            }
        }
    }

    final_set.sort();
    final_set.dedup();
    Ok(ResolvePlan {
        base: base.to_string(),
        components: final_set,
    })
}
