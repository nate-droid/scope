//! Line-based unified rendering, retaining line terminators for exact comparison.
use std::fmt::Write;

pub(super) fn unified(before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }
    let old: Vec<_> = before.split_inclusive('\n').collect();
    let new: Vec<_> = after.split_inclusive('\n').collect();
    let mut prefix = 0;
    while prefix < old.len().min(new.len()) && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len().min(new.len()) - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let a = &old[prefix..old.len() - suffix];
    let b = &new[prefix..new.len() - suffix];
    let mut ops: Vec<(char, &str)> = old[..prefix].iter().map(|line| (' ', *line)).collect();
    // Bound quadratic LCS storage/work. Large changed regions remain an exact,
    // valid (though not necessarily minimal) delete/insert hunk.
    if (a.len() + 1)
        .checked_mul(b.len() + 1)
        .is_some_and(|n| n <= 1_000_000)
    {
        let width = b.len() + 1;
        let mut lengths = vec![0usize; (a.len() + 1) * width];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lengths[i * width + j] = if a[i] == b[j] {
                    1 + lengths[(i + 1) * width + j + 1]
                } else {
                    lengths[(i + 1) * width + j].max(lengths[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                ops.push((' ', a[i]));
                i += 1;
                j += 1;
            } else if i < a.len()
                && (j == b.len() || lengths[(i + 1) * width + j] >= lengths[i * width + j + 1])
            {
                ops.push(('-', a[i]));
                i += 1;
            } else {
                ops.push(('+', b[j]));
                j += 1;
            }
        }
    } else {
        ops.extend(a.iter().map(|line| ('-', *line)));
        ops.extend(b.iter().map(|line| ('+', *line)));
    }
    ops.extend(old[old.len() - suffix..].iter().map(|line| (' ', *line)));
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for (index, (kind, _)) in ops.iter().enumerate() {
        if *kind == ' ' {
            continue;
        }
        let start = index.saturating_sub(3);
        let end = index.saturating_add(4).min(ops.len());
        if let Some(last) = hunks.last_mut().filter(|last| start <= last.1) {
            last.1 = end;
        } else {
            hunks.push((start, end));
        }
    }
    let mut output = String::from("--- before\n+++ after\n");
    let (mut cursor, mut old_line, mut new_line) = (0, 0, 0);
    for (start, end) in hunks {
        for (kind, _) in &ops[cursor..start] {
            old_line += usize::from(*kind != '+');
            new_line += usize::from(*kind != '-');
        }
        let old_count = ops[start..end]
            .iter()
            .filter(|(kind, _)| *kind != '+')
            .count();
        let new_count = ops[start..end]
            .iter()
            .filter(|(kind, _)| *kind != '-')
            .count();
        writeln!(
            output,
            "@@ -{},{} +{},{} @@",
            old_line + usize::from(old_count != 0),
            old_count,
            new_line + usize::from(new_count != 0),
            new_count
        )
        .unwrap();
        for (kind, line) in &ops[start..end] {
            output.push(*kind);
            output.push_str(line);
            if !line.ends_with('\n') {
                output.push_str("\n\\ No newline at end of file\n");
            }
            old_line += usize::from(*kind != '+');
            new_line += usize::from(*kind != '-');
        }
        cursor = end;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_context_and_no_newline() {
        assert_eq!(
            unified("head\né\nlast", "head\n🦀\nlast\n"),
            "--- before\n+++ after\n@@ -1,3 +1,3 @@\n head\n-é\n-last\n\\ No newline at end of file\n+🦀\n+last\n"
        );
        assert_eq!(
            unified("", "é"),
            "--- before\n+++ after\n@@ -0,0 +1,1 @@\n+é\n\\ No newline at end of file\n"
        );
        assert_eq!(
            unified("a\n", ""),
            "--- before\n+++ after\n@@ -1,1 +0,0 @@\n-a\n"
        );
        assert_eq!(unified("same", "same"), "");
    }

    #[test]
    fn separate_hunks_and_crlf() {
        let old = "0\r\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n";
        let new = old.replace("\n1\n", "\none\n").replace("\n10\n", "\nten\n");
        let result = unified(old, &new);
        assert_eq!(result.matches("@@ -").count(), 2);
        assert!(result.contains(" 0\r\n"));
        assert!(result.contains("-10\n+ten\n"));
    }
}
