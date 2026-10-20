//! Immutable output-view inputs. No semantic context or proof authority lives
//! here: source identity uses existing file URLs and authored syntax ranges.

use parser::TextRange;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrozenScalar {
    Bool(bool),
    Int {
        negative: bool,
        bytes: Vec<u8>,
        primitive: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenScopedImpl {
    pub trait_path: Vec<String>,
    pub target_path: Vec<String>,
    pub selection_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenFunctionTemplate {
    pub target_file: Url,
    /// Existing authored impl header receiving this method template.
    /// Syntax identity only; authority and semantic context are rechecked in output.
    pub request_impl: Option<TextRange>,
    /// Scoped on-demand derive impl header synthesized for a concrete `(Trait, Target, Provider)`.
    pub scoped_impl: Option<FrozenScopedImpl>,
    pub quote_range: TextRange,
    pub producer_module: Option<TextRange>,
    pub producer_block: Option<TextRange>,
    pub holes: Vec<FrozenScalar>,
}

#[salsa::input]
#[derive(Debug)]
pub struct GenerationInputs {
    #[return_ref]
    pub functions: Vec<FrozenFunctionTemplate>,
}
