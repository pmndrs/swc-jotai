#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::path::{Path, PathBuf};

use common::AtomImportMap;
use swc_plugin::{
    ast::*,
    metadata::{TransformPluginMetadataContextKind, TransformPluginProgramMetadata},
    plugin_transform,
    syntax_pos::DUMMY_SP,
    utils::{take::Take, ModuleItemLike, StmtLike, StmtOrModuleItem},
};

struct DebugLabelTransformVisitor {
    atom_import_map: AtomImportMap,
    current_var_declarator: Option<JsWord>,
    debug_label_expr: Option<Expr>,
    #[allow(dead_code)]
    path: PathBuf,
}

fn create_debug_label_assign_expr(atom_name: &JsWord) -> Expr {
    Expr::Assign(AssignExpr {
        left: PatOrExpr::Expr(Box::new(Expr::Member(MemberExpr {
            obj: Box::new(Expr::Ident(Ident {
                sym: atom_name.clone(),
                span: DUMMY_SP,
                optional: false,
            })),
            prop: MemberProp::Ident(Ident {
                sym: "debugLabel".into(),
                span: DUMMY_SP,
                optional: false,
            }),
            span: DUMMY_SP,
        }))),
        right: Box::new(Expr::Lit(Lit::Str(Str {
            value: atom_name.clone(),
            span: DUMMY_SP,
            raw: None,
        }))),
        op: op!("="),
        span: DUMMY_SP,
    })
}

impl DebugLabelTransformVisitor {
    pub fn new(path: &Path) -> Self {
        Self {
            atom_import_map: Default::default(),
            current_var_declarator: None,
            debug_label_expr: None,
            path: path.to_owned(),
        }
    }
}

impl DebugLabelTransformVisitor {
    fn visit_mut_stmt_like<T>(&mut self, stmts: &mut Vec<T>)
    where
        Vec<T>: VisitMutWith<Self>,
        T: VisitMutWith<Self> + StmtLike + ModuleItemLike + StmtOrModuleItem,
    {
        let mut stmts_updated: Vec<T> = Vec::with_capacity(stmts.len());

        for stmt in stmts.take() {
            let stmt = match stmt.into_stmt() {
                Ok(mut stmt) => {
                    stmt.visit_mut_with(self);
                    T::from_stmt(stmt)
                }
                Err(mut module_decl) => match module_decl {
                    ModuleDecl::ExportDefaultExpr(default_export) => {
                        if !self.atom_import_map.is_atom_import(&default_export.expr) {
                            continue;
                        }
                        // let atom_name: JsWord = self.path.file_name().unwrap().to_string_lossy().into();

                        // Variable declaration
                        stmts_updated.push(T::from_stmt(Stmt::Decl(Decl::Var(VarDecl {
                            declare: Default::default(),
                            decls: vec![VarDeclarator {
                                definite: false,
                                init: Some(default_export.expr),
                                name: Pat::Ident(Ident::new("countAtom".into(), DUMMY_SP).into()),
                                span: DUMMY_SP,
                            }],
                            kind: VarDeclKind::Const,
                            span: DUMMY_SP,
                        }))));
                        // Assign debug label
                        stmts_updated.push(T::from_stmt(Stmt::Expr(ExprStmt {
                            span: DUMMY_SP,
                            expr: Box::new(create_debug_label_assign_expr(&"countAtom".into())),
                        })));
                        // export default expression
                        stmts_updated.push(
                            T::try_from_module_decl(ModuleDecl::ExportDefaultExpr(
                                ExportDefaultExpr {
                                    expr: Box::new(Expr::Ident(Ident {
                                        sym: "countAtom".into(),
                                        span: DUMMY_SP,
                                        optional: false,
                                    })),
                                    span: DUMMY_SP,
                                },
                            ))
                            .unwrap(),
                        );
                        continue;
                    }
                    _ => {
                        module_decl.visit_mut_with(self);
                        T::try_from_module_decl(module_decl).unwrap()
                    }
                },
            };
            stmts_updated.push(stmt);

            if self.debug_label_expr.is_none() {
                continue;
            }

            stmts_updated.push(T::from_stmt(Stmt::Expr(ExprStmt {
                span: DUMMY_SP,
                expr: Box::new(self.debug_label_expr.take().unwrap()),
            })))
        }

        *stmts = stmts_updated;
    }
}

impl VisitMut for DebugLabelTransformVisitor {
    noop_visit_mut_type!();

    fn visit_mut_import_decl(&mut self, import: &mut ImportDecl) {
        self.atom_import_map.visit_import_decl(import);
    }

    fn visit_mut_var_declarator(&mut self, var_declarator: &mut VarDeclarator) {
        let old_var_declarator = self.current_var_declarator.take();

        self.current_var_declarator =
            if let Pat::Ident(BindingIdent { id, .. }) = &var_declarator.name {
                Some(id.sym.clone())
            } else {
                None
            };

        var_declarator.visit_mut_children_with(self);

        self.current_var_declarator = old_var_declarator;
    }

    fn visit_mut_call_expr(&mut self, call_expr: &mut CallExpr) {
        if self.current_var_declarator.is_none() {
            return;
        }

        call_expr.visit_mut_children_with(self);

        let atom_name = self.current_var_declarator.as_ref().unwrap();
        if let Callee::Expr(expr) = &call_expr.callee {
            if self.atom_import_map.is_atom_import(expr) {
                self.debug_label_expr = Some(create_debug_label_assign_expr(atom_name))
            }
        }
    }

    fn visit_mut_module_items(&mut self, items: &mut Vec<ModuleItem>) {
        self.visit_mut_stmt_like(items);
    }

    fn visit_mut_stmts(&mut self, stmts: &mut Vec<Stmt>) {
        self.visit_mut_stmt_like(stmts);
    }
}

#[plugin_transform]
pub fn debug_label_transform(
    program: Program,
    metadata: TransformPluginProgramMetadata,
) -> Program {
    let file_name = metadata
        .get_context(&TransformPluginMetadataContextKind::Filename)
        .unwrap_or_default()
        .replace("\\", "/");
    let path = Path::new(&file_name);
    program.fold_with(&mut as_folder(DebugLabelTransformVisitor::new(path)))
}

#[cfg(test)]
mod tests {
    use swc_ecma_parser::*;
    use swc_ecma_transforms_base::resolver;
    use swc_ecma_transforms_testing::test;
    use swc_plugin::{syntax_pos::Mark, *};

    use super::*;

    fn transform(path: Option<&Path>) -> impl Fold {
        chain!(
            resolver(Mark::new(), Mark::new(), false),
            as_folder(DebugLabelTransformVisitor::new(
                path.unwrap_or(&PathBuf::from("atoms.ts"))
            ))
        )
    }

    test!(
        Syntax::default(),
        |_| transform(None),
        basic,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        exported_atom,
        r#"
import { atom } from "jotai";
export const countAtom = atom(0);"#,
        r#"
import { atom } from "jotai";
export const countAtom = atom(0);
countAtom.debugLabel = "countAtom";"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        multiple_atoms,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
const doubleAtom = atom((get) => get(countAtom) * 2);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";
const doubleAtom = atom((get) => get(countAtom) * 2);
doubleAtom.debugLabel = "doubleAtom";"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        multiple_atoms_between_code,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
let counter = 0;
const increment = () => ++counter;
const doubleAtom = atom((get) => get(countAtom) * 2);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";
let counter = 0;
const increment = () => ++counter;
const doubleAtom = atom((get) => get(countAtom) * 2);
doubleAtom.debugLabel = "doubleAtom";"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        import_alias,
        r#"
import { atom as blah } from "jotai";
const countAtom = blah(0);"#,
        r#"
import { atom as blah } from "jotai";
const countAtom = blah(0);
countAtom.debugLabel = "countAtom";"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        ignore_non_jotai_imports,
        r#"
import React from "react";
import { atom } from "jotai";
import { defaultCount } from "./utils";
const countAtom = atom(0);"#,
        r#"
import React from "react";
import { atom } from "jotai";
import { defaultCount } from "./utils";      
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        namespace_import,
        r#"
import * as jotai from "jotai";
const countAtom = jotai.atom(0);"#,
        r#"
import * as jotai from "jotai";
const countAtom = jotai.atom(0);
countAtom.debugLabel = "countAtom";"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        atom_from_another_package,
        r#"
import { atom } from "some-library";
const countAtom = atom(0);"#,
        r#"
import { atom } from "some-library";
const countAtom = atom(0);"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        no_jotai_import,
        "const countAtom = atom(0);",
        "const countAtom = atom(0);"
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        ignore_default_export,
        r#"
import { atom } from "jotai";
export default atom(0);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";
export default countAtom;"#
    );

    test!(
        Syntax::default(),
        |_| transform(None),
        jotai_utils_import,
        r#"
import { atomWithImmer } from "jotai/immer";
import { atomWithMachine } from "jotai/xstate";
const immerAtom = atomWithImmer(0);
const toggleMachineAtom = atomWithMachine(() => toggleMachine);"#,
        r#"
import { atomWithImmer } from "jotai/immer";
import { atomWithMachine } from "jotai/xstate";
const immerAtom = atomWithImmer(0);
immerAtom.debugLabel = "immerAtom";
const toggleMachineAtom = atomWithMachine(() => toggleMachine);
toggleMachineAtom.debugLabel = "toggleMachineAtom";"#
    );
}
