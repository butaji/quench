#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::commands::build::source_gen::generate_all;

    /// Test the file-extension dispatch: a `.tsx` file with JSX should
    /// successfully generate Rust (not a `// Parse error:` stub), and a
    /// `.ts` file with plain TS should also succeed.
    #[test]
    fn generate_all_dispatches_tsx_vs_ts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tsx_path = dir.path().join("Counter.tsx");
        let ts_path = dir.path().join("helper.ts");
        fs::write(
            &tsx_path,
            r#"interface Props { initial?: number }
export default function Counter({ initial = 0 }: Props) {
  return <div class="counter">Count: {initial}</div>;
}
"#,
        )
        .expect("write tsx");
        fs::write(
            &ts_path,
            r#"export const answer: number = 42;
export function double(x: number): number { return x * 2; }
"#,
        )
        .expect("write ts");

        let result = generate_all(&[tsx_path.clone(), ts_path.clone()]).expect("generate_all");
        assert_eq!(result.generated_files.len(), 2);

        for gf in &result.generated_files {
            let body = &gf.content;
            assert!(
                !body.contains("// Parse error"),
                "{} should not be a parse error stub, got: {}",
                gf.path.display(),
                body
            );
        }
    }

    /// Edge case: a `.tsx` file that uses a `class` attribute (the exact
    /// pattern that previously failed when fed through parse_source instead
    /// of parse_tsx).
    #[test]
    fn generate_all_tsx_with_class_attribute_does_not_stub() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tsx_path = dir.path().join("Card.tsx");
        fs::write(
            &tsx_path,
            r#"export default function Card() {
  return <div class="card"><p class="title">hi</p></div>;
}
"#,
        )
        .expect("write");

        let result = generate_all(&[tsx_path]).expect("generate_all");
        assert_eq!(result.generated_files.len(), 1);
        assert!(
            !result.generated_files[0].content.contains("// Parse error"),
            "got: {}",
            result.generated_files[0].content
        );
    }

    /// When a file genuinely fails to parse, generate_all should still
    /// produce a `// Parse error:` stub for that file (so the build is
    /// complete and downstream code can reference it) — but other files
    /// in the batch should be unaffected.
    #[test]
    fn generate_all_parse_error_does_not_block_other_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bad = dir.path().join("bad.ts");
        let good = dir.path().join("good.ts");
        fs::write(&bad, "this is not valid typescript !!! @@@\n").expect("write bad");
        fs::write(&good, "export const x = 1;\n").expect("write good");

        let result = generate_all(&[bad.clone(), good.clone()]).expect("generate_all");
        assert_eq!(result.generated_files.len(), 2);
        let bad_stub = result
            .generated_files
            .iter()
            .find(|g| g.path.to_string_lossy().contains("bad"))
            .expect("bad stub");
        assert!(bad_stub.content.contains("// Parse error"));
        let good_out = result
            .generated_files
            .iter()
            .find(|g| g.path.to_string_lossy().contains("good"))
            .expect("good out");
        assert!(!good_out.content.contains("// Parse error"));
    }
}
