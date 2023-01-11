use std::collections::HashMap;

use egui::{Context, ScrollArea, SidePanel, Ui};
use fst::{
    fst::{Fst, HierarchyScope, ScopeId, VarId},
    valvec::ValAndTimeVec,
};
use log::info;

pub fn show_scopes_panel(ctx: &Context, e: &mut Fst, selected_scope: &mut Option<ScopeId>) {
    SidePanel::left("scopes_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Scopes");
            });

            ui.separator();

            // TODO: This will panic if there are no nodes.
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    show_hierarchy(ui, &e.hierarchy, ScopeId(0), selected_scope);
                });
        });
}

fn show_hierarchy(
    ui: &mut Ui,
    hierarchy: &espalier::Tree<ScopeId, HierarchyScope>,
    node_id: ScopeId,
    selected_id: &mut Option<ScopeId>,
) {
    let node = match hierarchy.get(node_id) {
        Some(n) => n,
        None => return,
    };

    let selected = Some(node_id) == *selected_id;

    // This is necessary because otherwise it uses the node.value.name as the ID
    // and there can be duplicates.
    ui.push_id(node_id, |ui| {
        if node.num_descendants() == 0 {
            if ui.selectable_label(selected, &node.value.name).clicked() {
                *selected_id = Some(node_id);
            }
        } else {
            let id = ui.make_persistent_id("scope_header");
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .show_header(ui, |ui| {
                    if ui.selectable_label(selected, &node.value.name).clicked() {
                        *selected_id = Some(node_id);
                    }
                })
                .body(|ui| {
                    for (child_id, _child) in hierarchy.children(node_id) {
                        show_hierarchy(ui, hierarchy, child_id, selected_id);
                    }
                });
        }
    });
}

pub fn show_vars_panel(
    ctx: &Context,
    e: &mut Fst,
    selected_scope: &Option<ScopeId>,
    vars_filter: &mut String,
    cached_waves: &mut HashMap<VarId, ValAndTimeVec>,
) {
    SidePanel::left("vars_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Vars");
            });

            ui.text_edit_singleline(vars_filter);

            ui.separator();

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(selected_scope) = selected_scope {
                        if let Some(scope) = e.hierarchy.get(*selected_scope) {
                            let append_var = show_vars(ui, &scope.value, vars_filter.as_str());

                            if let Some(varid) = append_var {
                                info!("Reading wave {:?}", varid);
                                // TODO: Do in another thread.
                                if let Ok(w) = e.read_wave(varid) {
                                    cached_waves.insert(varid, w);
                                }
                            }
                        }
                    }
                });
        });
}

fn show_vars(ui: &mut Ui, scope: &HierarchyScope, filter: &str) -> Option<VarId> {
    let mut add_var = None;
    for var in scope.vars.iter() {
        if var.name.contains(filter) {
            if ui.selectable_label(false, &var.name).double_clicked() {
                add_var = Some(var.id);
            }
        }
    }
    add_var
}
