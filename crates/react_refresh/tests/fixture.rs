use std::path::PathBuf;

use swc_core::{
    common::{chain, Mark},
    ecma::transforms::{
        base::resolver,
        react::{react, Options, RefreshOptions},
        testing::test_fixture,
    },
};
use swc_ecma_parser::{EsConfig, Syntax};
use swc_jotai_react_refresh::react_refersh;
use testing::fixture;

#[fixture("tests/fixtures/**/input.js")]
fn test(input: PathBuf) {
    let output = input.with_file_name("output.js");

    test_fixture(
        Syntax::Es(EsConfig {
            jsx: true,
            ..Default::default()
        }),
        &|t| {
            let unresolved_mark = Mark::new();
            let top_level_mark = Mark::new();

            chain!(
                resolver(unresolved_mark, top_level_mark, false),
                react_refersh(&PathBuf::from("atoms.ts")),
                react(
                    t.cm.clone(),
                    Some(t.comments.clone(),),
                    Options {
                        development: Some(true),
                        refresh: Some(RefreshOptions {
                            refresh_reg: "$___refreshReg$".into(),
                            refresh_sig: "$___refreshSig$".into(),
                            emit_full_signatures: false
                        }),
                        ..Default::default()
                    },
                    top_level_mark
                ),
                swc_ecma_transforms_compat::es2015(
                    unresolved_mark,
                    Some(t.comments.clone()),
                    Default::default()
                ),
            )
        },
        &input,
        &output,
        Default::default(),
    )
}
