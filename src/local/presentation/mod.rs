mod build;
mod diff;
mod input;
mod model;
mod render;
mod sanitize;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod v2_tests;

pub use build::build_presentation;
pub(crate) use build::build_presentation_inputs;
pub(crate) use input::{
    PresentationInput, PresentationOrphanPollInput, PresentationPollAggregateInput,
};
pub(crate) use model::PRESENTATION_RAW_INVOCATION_ID_SAMPLE_LIMIT;
pub use model::{
    PRESENTATION_SCHEMA_VERSION, PresentationAgent, PresentationDiffLine, PresentationDocument,
    PresentationEvidence, PresentationFileChange, PresentationFileOperation, PresentationKind,
    PresentationPollSummary, PresentationRecord, PresentationWorkdir, PresentationWriteMode,
};
pub use render::render_presentation;
pub(crate) use sanitize::{StreamingDisplaySanitizer, markdown_code_span, sanitize_display_text};
