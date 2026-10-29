use std::rc::Rc;

use crate::buffer::Buffer;
use crate::completion::Icon;
use crate::editor::Editor;

/// How a plugin's buffer is shown where buffers are listed or named, in place of its name,
/// which commands still go by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BufferLook {
    pub icon: Option<Icon>,
    pub label: String,
    /// Listed apart from the other buffers, after them, e.g. `Terminals`.
    pub group: Option<&'static str>,
    /// After the label, e.g. where it is.
    pub annotation: String,
    /// At the right, e.g. whether it runs.
    pub detail: Option<String>,
}

/// Describes a buffer of the plugin's; the first that knows the buffer does.
pub type BufferLookProvider = Rc<dyn Fn(&Editor, &Buffer) -> Option<BufferLook>>;
