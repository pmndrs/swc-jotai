#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use common::{convert_path_to_posix, AtomImportMap};
use swc_core::{
    common::util::take::Take,
    common::DUMMY_SP,
    ecma::{
        ast::*,
        atoms::JsWord,
        utils::{ModuleItemLike, StmtLike, StmtOrModuleItem},
        visit::{as_folder, noop_visit_mut_type, FoldWith, VisitMut, VisitMutWith},
    },
    plugin::{
        metadata::TransformPluginMetadataContextKind, plugin_transform,
        proxies::TransformPluginProgramMetadata,
    },
    quote,
};

pub struct ReactRefreshTransformVisitor {
    atom_import_map: AtomImportMap,
    current_var_declarator: Option<Id>,
    refresh_atom_var_decl: Option<VarDeclarator>,
    #[allow(dead_code)]
    path: PathBuf,
}

fn create_react_refresh_call_expr(key: String, atom_expr: &CallExpr) -> Box<Expr> {
    Box::new(Expr::Call(CallExpr {
        span: DUMMY_SP,
        callee: Callee::Expr(Box::new(Expr::Member(MemberExpr {
            span: DUMMY_SP,
            obj: Box::new(Expr::Member(MemberExpr {
                span: DUMMY_SP,
                obj: Box::new(Expr::Ident(Ident::new("globalThis".into(), DUMMY_SP))),
                prop: MemberProp::Ident(Ident::new("jotaiAtomCache".into(), DUMMY_SP)),
            })),
            prop: MemberProp::Ident(Ident::new("get".into(), DUMMY_SP)),
        }))),
        args: vec![
            ExprOrSpread {
                spread: None,
                expr: Box::new(Expr::Lit(Lit::Str(Str {
                    value: key.into(),
                    span: DUMMY_SP,
                    raw: None,
                }))),
            },
            ExprOrSpread {
                spread: None,
                expr: Box::new(Expr::Call(atom_expr.clone())),
            },
        ],
        type_args: None,
    }))
}

fn create_react_refresh_var_decl(
    atom_name_id: Id,
    key: String,
    atom_expr: &CallExpr,
) -> VarDeclarator {
    let atom_name = atom_name_id.0.clone();
    VarDeclarator {
        name: Pat::Ident(Ident::new(atom_name, DUMMY_SP.with_ctxt(atom_name_id.1)).into()),
        span: DUMMY_SP.with_ctxt(atom_name_id.1),
        init: Some(create_react_refresh_call_expr(key, atom_expr)),
        definite: false,
    }
}

fn create_cache_key(atom_name: &JsWord, path: &PathBuf) -> String {
    path.clone().display().to_string() + "/" + &atom_name.clone().to_string().to_owned()
}

impl ReactRefreshTransformVisitor {
    pub fn new(path: &Path) -> Self {
        Self {
            atom_import_map: Default::default(),
            current_var_declarator: None,
            refresh_atom_var_decl: None,
            path: path.to_owned(),
        }
    }

    fn visit_mut_stmt_like<T>(&mut self, stmts: &mut Vec<T>)
    where
        Vec<T>: VisitMutWith<Self>,
        T: VisitMutWith<Self> + StmtLike + ModuleItemLike + StmtOrModuleItem,
    {
        let mut stmts_updated: Vec<T> = Vec::with_capacity(stmts.len());
        let mut is_atom_present: bool = false;

        for stmt in stmts.take() {
            let stmt = match stmt.into_stmt() {
                Ok(mut stmt) => {
                    stmt.visit_mut_with(self);
                    <T as StmtLike>::from_stmt(stmt)
                }
                Err(mut module_decl) => match module_decl {
                    ModuleDecl::ExportDefaultExpr(mut default_export) => {
                        if !self.atom_import_map.is_atom_import(&default_export.expr) {
                            default_export.visit_mut_with(self);
                            stmts_updated.push(
                                <T as ModuleItemLike>::try_from_module_decl(default_export.into())
                                    .unwrap(),
                            );
                            continue;
                        }
                        is_atom_present = true;

                        let atom_name: JsWord = self
                            .path
                            .file_stem()
                            .unwrap_or_else(|| OsStr::new("default_atom"))
                            .to_string_lossy()
                            .into();

                        // export default expression
                        stmts_updated.push(
                            <T as ModuleItemLike>::try_from_module_decl(
                                ModuleDecl::ExportDefaultExpr(ExportDefaultExpr {
                                    expr: create_react_refresh_call_expr(
                                        create_cache_key(&atom_name, &self.path),
                                        default_export.expr.as_call().unwrap(),
                                    ),
                                    span: DUMMY_SP,
                                }),
                            )
                            .unwrap(),
                        );
                        continue;
                    }
                    _ => {
                        module_decl.visit_mut_with(self);
                        <T as ModuleItemLike>::try_from_module_decl(module_decl).unwrap()
                    }
                },
            };

            if self.refresh_atom_var_decl.is_none() {
                stmts_updated.push(stmt);
                continue;
            }

            is_atom_present = true;

            stmts_updated.push(<T as StmtOrModuleItem>::from_stmt(Stmt::Decl(Decl::Var(
                Box::new(VarDecl {
                    span: DUMMY_SP,
                    kind: VarDeclKind::Const,
                    declare: false,
                    decls: vec![self.refresh_atom_var_decl.take().unwrap()],
                }),
            ))))
        }

        if is_atom_present {
            let jotai_cache_stmt = quote!(
                "globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
                cache: new Map(),
                get(name, inst) { 
                  if (this.cache.has(name)) {
                    return this.cache.get(name)
                  }
                  this.cache.set(name, inst)
                  return inst
                },
              }" as Stmt
            );
            let mut stmts_with_cache: Vec<T> = Vec::with_capacity(stmts_updated.len() + 1);
            stmts_with_cache.push(<T as StmtLike>::from_stmt(jotai_cache_stmt));
            stmts_with_cache.append(&mut stmts_updated);
            stmts_updated = stmts_with_cache
        }

        *stmts = stmts_updated;
    }
}

impl VisitMut for ReactRefreshTransformVisitor {
    noop_visit_mut_type!();

    fn visit_mut_import_decl(&mut self, import: &mut ImportDecl) {
        self.atom_import_map.visit_import_decl(import);
    }

    fn visit_mut_var_declarator(&mut self, var_declarator: &mut VarDeclarator) {
        let old_var_declarator = self.current_var_declarator.take();

        self.current_var_declarator = if let Pat::Ident(BindingIdent {
            id: Ident { span, sym, .. },
            ..
        }) = &var_declarator.name
        {
            Some((sym.clone(), span.ctxt))
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
                self.refresh_atom_var_decl = Some(create_react_refresh_var_decl(
                    atom_name.clone(),
                    create_cache_key(&atom_name.0, &self.path),
                    call_expr,
                ))
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
pub fn react_refresh_transform(
    program: Program,
    metadata: TransformPluginProgramMetadata,
) -> Program {
    let file_name = convert_path_to_posix(
        &metadata
            .get_context(&TransformPluginMetadataContextKind::Filename)
            .unwrap_or_default(),
    );
    let path = Path::new(&file_name);
    program.fold_with(&mut as_folder(ReactRefreshTransformVisitor::new(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swc_core::{
        common::{chain, Mark},
        ecma::{
            transforms::{base::resolver, testing::test},
            visit::{as_folder, Fold},
        },
    };
    use swc_ecma_parser::Syntax;

    fn transform(path: Option<&Path>) -> impl Fold {
        chain!(
            resolver(Mark::new(), Mark::new(), false),
            as_folder(ReactRefreshTransformVisitor::new(
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
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom } from "jotai";
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));"#
    );
}
