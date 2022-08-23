use swc_common::collections::AHashSet;
use swc_plugin::ast::*;

#[derive(Debug)]
pub struct AtomImportMap {
    imports: AHashSet<JsWord>,
    namespace_imports: AHashSet<JsWord>,
}

impl AtomImportMap {
    pub fn visit_import_decl(&mut self, import: &ImportDecl) {
        for s in &import.specifiers {
            let local_ident = match s {
                ImportSpecifier::Named(ImportNamedSpecifier {
                    local, imported, ..
                }) => match imported {
                    Some(imported) => {
                        if let ModuleExportName::Ident(v) = imported {
                            v.sym.clone()
                        } else {
                            continue;
                        }
                    }
                    _ => local.sym.clone(),
                },
                ImportSpecifier::Namespace(..) => {
                    self.namespace_imports.insert(import.src.value.clone());
                    continue;
                }
                _ => continue,
            };

            self.imports.insert(local_ident);
        }
    }

    pub fn is_atom_import(&self, expr: &Expr, ident: &str) -> bool {
        match expr {
            Expr::Ident(i) => {
                if let Some(i_sym) = self.imports.get(&i.sym) {
                    i_sym == ident
                } else {
                    false
                }
            }
            Expr::Member(MemberExpr {
                obj,
                prop: MemberProp::Ident(prop),
                ..
            }) => {
                if let Expr::Ident(obj) = &**obj {
                    if let Some(..) = self.namespace_imports.get(&obj.sym) {
                        return prop.sym == *ident;
                    }
                }
                false
            }
            _ => false,
        }
    }
}
