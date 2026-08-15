mod build;
mod diff;
mod model;
mod render;
mod sanitize;

#[cfg(test)]
mod tests;

pub use build::build_presentation;
pub use model::{
    PRESENTATION_SCHEMA_VERSION, PresentationAgent, PresentationDiffLine, PresentationDocument,
    PresentationEvidence, PresentationFileChange, PresentationFileOperation, PresentationKind,
    PresentationPollSummary, PresentationRecord, PresentationWorkdir, PresentationWriteMode,
};
pub use render::render_presentation;
pub(crate) use sanitize::{markdown_code_span, sanitize_display_text};
