use swc_common::collections::AHashSet;
use swc_plugin::ast::*;

use crate::ATOM_IMPORTS;

#[derive(Debug, Default)]
pub struct AtomImportMap {
    imports: AHashSet<JsWord>,
    namespace_imports: AHashSet<JsWord>,
}

impl AtomImportMap {
    pub fn visit_import_decl(&mut self, import: &ImportDecl) {
        if !import.src.value.starts_with("jotai") {
            return;
        }

        for s in &import.specifiers {
            let local_ident = match s {
                ImportSpecifier::Named(ImportNamedSpecifier {
                    local,
                    imported: Some(ModuleExportName::Ident(ident)),
                    ..
                }) => {
                    if ATOM_IMPORTS.contains(&&*ident.sym) {
                        local.sym.clone()
                    } else {
                        continue;
                    }
                }
                ImportSpecifier::Named(ImportNamedSpecifier { local, .. }) => {
                    if ATOM_IMPORTS.contains(&&*local.sym) {
                        local.sym.clone()
                    } else {
                        continue;
                    }
                }
                ImportSpecifier::Namespace(..) => {
                    self.namespace_imports.insert(import.src.value.clone());
                    continue;
                }
                _ => continue,
            };

            self.imports.insert(local_ident);
        }
    }

    pub fn is_atom_import(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ident(i) => self.imports.get(&i.sym).is_some(),
            Expr::Member(MemberExpr {
                obj,
                prop: MemberProp::Ident(prop),
                ..
            }) => {
                if let Expr::Ident(obj) = &**obj {
                    if let Some(..) = self.namespace_imports.get(&obj.sym) {
                        return ATOM_IMPORTS.contains(&&*prop.sym);
                    }
                }
                false
            }
            _ => false,
        }
    }
}
