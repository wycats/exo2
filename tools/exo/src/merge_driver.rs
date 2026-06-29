use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeDriverKind {
    Toml,
}

pub fn run(
    kind: MergeDriverKind,
    base: &Path,
    current: &Path,
    other: &Path,
    path: Option<&str>,
) -> i32 {
    let result = match kind {
        MergeDriverKind::Toml => run_toml(base, current, other, path),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("exo merge-driver failed: {err}");
            2
        }
    }
}

fn run_toml(base: &Path, current: &Path, other: &Path, _logical_path: Option<&str>) -> Result<i32> {
    let base_s = read_to_string_lossy(base).unwrap_or_default();
    let current_s = read_to_string_lossy(current).unwrap_or_default();
    let other_s = read_to_string_lossy(other).unwrap_or_default();

    let base_v = parse_toml_or_empty(&base_s)?;
    let current_v = parse_toml_or_empty(&current_s)?;
    let other_v = parse_toml_or_empty(&other_s)?;

    let (merged, conflicted) = merge_value(&base_v, &current_v, &other_v);

    if !conflicted {
        let rendered = toml::to_string_pretty(&merged).context("Failed to render merged TOML")?;
        fs::write(current, rendered).with_context(|| {
            format!(
                "Failed to write merged TOML back to current file at {}",
                current.display()
            )
        })?;
        return Ok(0);
    }

    // Fall back to git's normal textual merge with conflict markers.
    let merged_text =
        git_merge_file(current, base, other).context("Failed to fall back to `git merge-file`")?;
    fs::write(current, merged_text).with_context(|| {
        format!(
            "Failed to write conflict-marked merge result back to current file at {}",
            current.display()
        )
    })?;

    Ok(1)
}

fn read_to_string_lossy(path: &Path) -> Option<String> {
    // Git may pass a non-existent base (e.g. /dev/null-ish paths). Treat missing as empty.
    fs::read_to_string(path).ok()
}

fn parse_toml_or_empty(input: &str) -> Result<toml::Value> {
    if input.trim().is_empty() {
        return Ok(toml::Value::Table(toml::map::Map::new()));
    }

    toml::from_str::<toml::Value>(input).context("Failed to parse TOML")
}

fn git_merge_file(current: &Path, base: &Path, other: &Path) -> Result<String> {
    use std::process::Command;

    let output = Command::new("git")
        .args([
            "merge-file",
            "-p",
            "--diff3",
            current
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("current path was not utf8"))?,
            base.to_str()
                .ok_or_else(|| anyhow::anyhow!("base path was not utf8"))?,
            other
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("other path was not utf8"))?,
        ])
        .output()
        .context("Failed to execute git merge-file")?;

    // git merge-file returns:
    // - 0 if merged cleanly
    // - 1 if conflicts
    // - >1 on error
    if output.status.code().is_none() {
        anyhow::bail!("git merge-file terminated by signal");
    }

    if output.status.code().is_some_and(|c| c > 1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git merge-file failed: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn merge_value(
    base: &toml::Value,
    current: &toml::Value,
    other: &toml::Value,
) -> (toml::Value, bool) {
    if current == other {
        return (current.clone(), false);
    }
    if current == base {
        return (other.clone(), false);
    }
    if other == base {
        return (current.clone(), false);
    }

    match (base, current, other) {
        (toml::Value::Table(b), toml::Value::Table(a), toml::Value::Table(c)) => {
            let mut conflicted = false;
            let mut out: toml::map::Map<String, toml::Value> = toml::map::Map::new();
            let empty = toml::Value::Table(toml::map::Map::new());

            let mut keys: BTreeSet<&String> = BTreeSet::new();
            keys.extend(b.keys());
            keys.extend(a.keys());
            keys.extend(c.keys());

            for key in keys {
                let b_v = b.get(key).unwrap_or(&empty);
                let a_opt = a.get(key);
                let c_opt = c.get(key);

                match (a_opt, c_opt) {
                    (Some(a_v), Some(c_v)) => {
                        let (merged, did_conflict) = merge_value(b_v, a_v, c_v);
                        conflicted |= did_conflict;
                        out.insert(key.clone(), merged);
                    }
                    (Some(a_v), None) => {
                        // Deletion vs change: if other deleted but current differs from base, treat as conflict.
                        if let Some(b_orig) = b.get(key) {
                            if a_v == b_orig {
                                // other deleted, current unchanged => delete
                            } else {
                                conflicted = true;
                                out.insert(key.clone(), a_v.clone());
                            }
                        } else {
                            out.insert(key.clone(), a_v.clone());
                        }
                    }
                    (None, Some(c_v)) => {
                        if let Some(b_orig) = b.get(key) {
                            if c_v == b_orig {
                                // current deleted, other unchanged => delete
                            } else {
                                conflicted = true;
                                out.insert(key.clone(), c_v.clone());
                            }
                        } else {
                            out.insert(key.clone(), c_v.clone());
                        }
                    }
                    (None, None) => {}
                }
            }

            (toml::Value::Table(out), conflicted)
        }
        (toml::Value::Array(b), toml::Value::Array(a), toml::Value::Array(c)) => {
            if let Some((out, conflicted)) = merge_array_of_tables_by_id(b, a, c) {
                (toml::Value::Array(out), conflicted)
            } else {
                (current.clone(), true)
            }
        }
        _ => (current.clone(), true),
    }
}

fn merge_array_of_tables_by_id(
    base: &[toml::Value],
    current: &[toml::Value],
    other: &[toml::Value],
) -> Option<(Vec<toml::Value>, bool)> {
    let (base_map, base_order) = index_array_of_tables_by_id(base)?;
    let (current_map, current_order) = index_array_of_tables_by_id(current)?;
    let (other_map, other_order) = index_array_of_tables_by_id(other)?;

    let mut ids: BTreeSet<String> = BTreeSet::new();
    ids.extend(base_map.keys().cloned());
    ids.extend(current_map.keys().cloned());
    ids.extend(other_map.keys().cloned());

    let mut keep: BTreeMap<String, Option<toml::Value>> = BTreeMap::new();
    let mut conflicted = false;

    for id in ids {
        let b = base_map.get(&id);
        let a = current_map.get(&id);
        let c = other_map.get(&id);

        let chosen: Option<toml::Value> = match (b, a, c) {
            (_, Some(a_v), Some(c_v)) => {
                let b_v = b.unwrap_or(a_v);
                let (merged, did_conflict) = merge_value(b_v, a_v, c_v);
                conflicted |= did_conflict;
                Some(merged)
            }
            (Some(b_v), Some(a_v), None) => {
                if a_v == b_v {
                    None
                } else {
                    conflicted = true;
                    Some(a_v.clone())
                }
            }
            (Some(b_v), None, Some(c_v)) => {
                if c_v == b_v {
                    None
                } else {
                    conflicted = true;
                    Some(c_v.clone())
                }
            }
            (None, Some(a_v), None) => Some(a_v.clone()),
            (None, None, Some(c_v)) => Some(c_v.clone()),
            (_, None, None) => None,
        };

        keep.insert(id, chosen);
    }

    let mut out = Vec::new();

    for id in current_order {
        if let Some(Some(v)) = keep.get(&id) {
            out.push(v.clone());
        }
    }

    for id in other_order {
        if current_map.contains_key(&id) {
            continue;
        }
        if let Some(Some(v)) = keep.get(&id) {
            out.push(v.clone());
        }
    }

    // If base introduced ids that neither side had in order, append deterministically.
    for id in base_order {
        if current_map.contains_key(&id) || other_map.contains_key(&id) {
            continue;
        }
        if let Some(Some(v)) = keep.get(&id) {
            out.push(v.clone());
        }
    }

    Some((out, conflicted))
}

fn index_array_of_tables_by_id(
    arr: &[toml::Value],
) -> Option<(BTreeMap<String, toml::Value>, Vec<String>)> {
    let mut map = BTreeMap::new();
    let mut order = Vec::new();

    for v in arr {
        let toml::Value::Table(t) = v else {
            return None;
        };

        let id = t.get("id")?;
        let id = id.as_str()?.to_string();

        order.push(id.clone());
        map.insert(id, toml::Value::Table(t.clone()));
    }

    Some((map, order))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn make_ideas_doc(
        base_ids_in_order: &[u8],
        add_ids_in_order: &[u8],
        add_value_offset: i64,
    ) -> toml::Value {
        let mut doc = toml::map::Map::<String, toml::Value>::new();
        let mut ideas = Vec::new();

        for id in base_ids_in_order.iter().copied() {
            let mut t = toml::map::Map::<String, toml::Value>::new();
            t.insert("id".to_string(), toml::Value::String(format!("id-{id}")));
            // Base entries must be identical across base/current/other for
            // the "additions-only" proptest.
            t.insert("value".to_string(), toml::Value::Integer(id as i64));
            ideas.push(toml::Value::Table(t));
        }

        for id in add_ids_in_order.iter().copied() {
            let mut t = toml::map::Map::<String, toml::Value>::new();
            t.insert("id".to_string(), toml::Value::String(format!("id-{id}")));
            t.insert(
                "value".to_string(),
                toml::Value::Integer(add_value_offset + id as i64),
            );
            ideas.push(toml::Value::Table(t));
        }

        doc.insert("ideas".to_string(), toml::Value::Array(ideas));
        toml::Value::Table(doc)
    }

    fn extract_ideas_ids(doc: &toml::Value) -> Vec<String> {
        let ideas = doc
            .as_table()
            .and_then(|t| t.get("ideas"))
            .and_then(toml::Value::as_array)
            .expect("expected doc.ideas to be an array");

        ideas
            .iter()
            .map(|v| {
                v.as_table()
                    .and_then(|t| t.get("id"))
                    .and_then(toml::Value::as_str)
                    .expect("expected ideas[*].id to be a string")
                    .to_string()
            })
            .collect()
    }

    fn unique_u8_vec(max_len: usize) -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(0u8..40u8, 0..=max_len).prop_filter("ids must be unique", |v| {
            let mut seen = BTreeSet::new();
            v.iter().all(|x| seen.insert(*x))
        })
    }

    proptest! {
        #[test]
        fn proptest_union_additions_only_by_id(
            base_ids in unique_u8_vec(8),
            current_add in unique_u8_vec(6),
            other_add in unique_u8_vec(6),
        ) {
            let base_set: BTreeSet<u8> = base_ids.iter().copied().collect();
            let current_add_set: BTreeSet<u8> = current_add.iter().copied().collect();
            let other_add_set: BTreeSet<u8> = other_add.iter().copied().collect();

            // Ensure additions are disjoint from each other and from base.
            prop_assume!(base_set.is_disjoint(&current_add_set));
            prop_assume!(base_set.is_disjoint(&other_add_set));
            prop_assume!(current_add_set.is_disjoint(&other_add_set));

            let base = make_ideas_doc(&base_ids, &[], 1000);
            let current = make_ideas_doc(&base_ids, &current_add, 2000);
            let other = make_ideas_doc(&base_ids, &other_add, 3000);

            let (merged, conflicted) = merge_value(&base, &current, &other);
            prop_assert!(!conflicted);

            let merged_ids = extract_ideas_ids(&merged);

            // Ordering should follow `current` first, then new ids from `other`.
            let mut expected_order: Vec<String> = Vec::new();
            expected_order.extend(base_ids.iter().map(|id| format!("id-{id}")));
            expected_order.extend(current_add.iter().map(|id| format!("id-{id}")));
            expected_order.extend(other_add.iter().map(|id| format!("id-{id}")));
            prop_assert_eq!(&merged_ids, &expected_order);

            // No duplicates.
            let merged_set: BTreeSet<String> = merged_ids.iter().cloned().collect();
            prop_assert_eq!(merged_set.len(), merged_ids.len());

            // Swapping sides should not change the resulting id *set*.
            let (merged_swapped, conflicted_swapped) = merge_value(&base, &other, &current);
            prop_assert!(!conflicted_swapped);
            let swapped_set: BTreeSet<String> = extract_ideas_ids(&merged_swapped).into_iter().collect();
            prop_assert_eq!(merged_set, swapped_set);
        }

        #[test]
        fn proptest_short_circuits(
            ids in unique_u8_vec(10)
        ) {
            let base = make_ideas_doc(&ids, &[], 1000);
            let current = make_ideas_doc(&ids, &[41], 2000);
            let other = make_ideas_doc(&ids, &[42], 3000);

            // If current==other, we must take it without conflict.
            let (m1, c1) = merge_value(&base, &current, &current);
            prop_assert!(!c1);
            prop_assert_eq!(m1, current.clone());

            // If current==base, take other without conflict.
            let (m2, c2) = merge_value(&current, &current, &other);
            prop_assert!(!c2);
            prop_assert_eq!(m2, other.clone());

            // If other==base, take current without conflict.
            let (m3, c3) = merge_value(&other, &current, &other);
            prop_assert!(!c3);
            prop_assert_eq!(m3, current);
        }
    }

    #[test]
    fn merges_array_of_tables_by_id_union_additions() {
        let base = toml::from_str::<toml::Value>(
            r#"[[ideas]]
id = "a"
value = 1
"#,
        )
        .unwrap();
        let current = toml::from_str::<toml::Value>(
            r#"[[ideas]]
id = "a"
value = 1

[[ideas]]
id = "b"
value = 2
"#,
        )
        .unwrap();
        let other = toml::from_str::<toml::Value>(
            r#"[[ideas]]
id = "a"
value = 1

[[ideas]]
id = "c"
value = 3
"#,
        )
        .unwrap();

        let (merged, conflicted) = merge_value(&base, &current, &other);
        assert!(!conflicted);

        let s = toml::to_string(&merged).unwrap();
        assert!(s.contains("id = \"b\""));
        assert!(s.contains("id = \"c\""));
    }
}
