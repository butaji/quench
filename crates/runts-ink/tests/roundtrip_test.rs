use rquickjs::{Context, Runtime};

#[test]
fn box_props_roundtrip() {
    let rt = Runtime::new().unwrap();
    let ctx = Context::full(&rt).unwrap();
    ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx).unwrap();

        let obj: rquickjs::Object = ctx
            .eval(
                r#"
                runts_ink.box({
                    paddingX: 2,
                    paddingY: 1,
                    borderStyle: "single",
                    children: []
                })
            "#,
            )
            .unwrap();

        let inner: rquickjs::Object = obj.get("Box").unwrap();
        let props: rquickjs::Object = inner.get("__props").unwrap();

        let pt: i32 = props.get("paddingTop").unwrap_or(-1);
        let pl: i32 = props.get("paddingLeft").unwrap_or(-1);
        let bt: bool = props.get("borderTop").unwrap_or(false);
        let bs: String = props.get("borderStyle").unwrap_or_default();

        eprintln!("pt={} pl={} bt={} bs={}", pt, pl, bt, bs);

        assert_eq!(pt, 1, "paddingTop should be 1");
        assert_eq!(pl, 2, "paddingLeft should be 2");
        assert!(bt, "borderTop should be true");
        assert_eq!(bs, "single", "borderStyle should be single");
    });
}
