use crate::context::ContextHandle;
use crate::keymap::KeymapHandle;
use kbvm::xkb::diagnostic::Diagnostic;
use kbvm::xkb::diagnostic::DiagnosticHandler;
use kbvm::xkb::diagnostic::DiagnosticKind;
use kbvm::xkb::diagnostic::WriteToLog;
use kbvm::xkb::rmlvo::Group;
use std::sync::Arc;

pub(crate) const BUILTIN_US_KEYMAP: &str = include_str!("../data/us.xkb");

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RuleNames<'a> {
    pub(crate) rules: Option<&'a str>,
    pub(crate) model: Option<&'a str>,
    pub(crate) layout: Option<&'a str>,
    pub(crate) variant: Option<&'a str>,
    pub(crate) options: Option<&'a str>,
}

#[derive(Default)]
struct RulesLookup {
    missing_file: bool,
}

impl DiagnosticHandler for RulesLookup {
    fn filter(&self, kind: DiagnosticKind, is_fatal: bool) -> bool {
        kind == DiagnosticKind::FileNotFound || WriteToLog.filter(kind, is_fatal)
    }

    fn handle(&mut self, diag: Diagnostic) {
        if diag.kind() == DiagnosticKind::FileNotFound {
            self.missing_file = true;
        }
        if WriteToLog.filter(diag.kind(), false) {
            WriteToLog.handle(diag);
        }
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

pub(crate) fn keymap_from_names(context: Arc<ContextHandle>, names: RuleNames<'_>) -> KeymapHandle {
    let kbvm_context = context.kbvm_context();
    let rules = non_empty(names.rules);
    let model = non_empty(names.model);
    let groups: Option<Vec<Group<'_>>> = non_empty(names.layout).map(|layout| {
        Group::from_layouts_and_variants(layout, names.variant.unwrap_or_default()).collect()
    });
    let options: Option<Vec<&str>> = names.options.map(|options| options.split(',').collect());
    let mut lookup = RulesLookup::default();
    kbvm_context.expand_names(
        &mut lookup,
        rules,
        model,
        groups.as_deref(),
        options.as_deref(),
    );
    if lookup.missing_file {
        eprintln!(
            "xkbcommon: WARNING: XKB rules {} not found in the include paths; \
             using the built-in evdev/pc105/us keymap",
            rules.map_or_else(|| "(default)".to_string(), |r| format!("'{r}'")),
        );
        return KeymapHandle::new(context, BUILTIN_US_KEYMAP.as_bytes())
            .expect("the built-in US keymap compiles");
    }
    let keymap = kbvm_context.keymap_from_names(
        WriteToLog,
        rules,
        model,
        groups.as_deref(),
        options.as_deref(),
    );
    let text = keymap.format().to_string();
    KeymapHandle::from_compiled(context, keymap, text.as_bytes())
}
