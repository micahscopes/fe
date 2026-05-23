use std::slice;

crate::define_closed_string_enum! {
    /// Coarse reason an origin edge exists.
    ///
    /// Exporters can later project these into more specific relation names, but the
    /// core graph should preserve enough intent that a link is not just "related".
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update)]
    pub enum OriginLinkKind {
        /// The target is the next compiler representation of the source.
        Lowered => "lowered",
        /// The target was created while expanding or desugaring the source.
        Expanded => "expanded",
        /// The target was created by a transform or optimization pass.
        Transformed => "transformed",
        /// The target was introduced by the compiler and has no direct source node.
        Synthetic => "synthetic",
        /// The target preserves the source identity in another namespace.
        Alias => "alias",
    }
}

/// Directed edge in an origin graph.
///
/// Direction is earlier artifact to later artifact:
///
/// ```text
/// HIR -> semantic -> MIR -> backend -> bytecode
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OriginLink<Node> {
    from: Node,
    to: Node,
    kind: OriginLinkKind,
}

// SAFETY: `OriginLink` contains only two origin nodes and a link-kind enum.
// Fieldwise `salsa::Update` keeps cached graphs precise while preserving each
// node type's own update invariants.
unsafe impl<Node> salsa::Update for OriginLink<Node>
where
    Node: salsa::Update,
{
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let mut changed = false;
        unsafe {
            changed |= Node::maybe_update(&mut (*old_pointer).from, new_value.from);
            changed |= Node::maybe_update(&mut (*old_pointer).to, new_value.to);
            changed |= OriginLinkKind::maybe_update(&mut (*old_pointer).kind, new_value.kind);
        }
        changed
    }
}

impl<Node> OriginLink<Node> {
    pub const fn new(from: Node, to: Node, kind: OriginLinkKind) -> Self {
        Self { from, to, kind }
    }

    pub fn from(&self) -> &Node {
        &self.from
    }

    pub fn to(&self) -> &Node {
        &self.to
    }

    pub const fn kind(&self) -> OriginLinkKind {
        self.kind
    }

    pub fn into_parts(self) -> (Node, Node, OriginLinkKind) {
        (self.from, self.to, self.kind)
    }
}

/// Immutable origin graph container returned by queries and consumed by exports.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OriginGraph<Node> {
    links: Vec<OriginLink<Node>>,
}

// SAFETY: `OriginGraph` is an owned vector of `OriginLink<Node>`. The vector
// update implementation delegates element updates to `OriginLink<Node>`, so no
// graph-specific aliasing or database lifetime assumptions are introduced here.
unsafe impl<Node> salsa::Update for OriginGraph<Node>
where
    Node: salsa::Update,
{
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        unsafe { Vec::<OriginLink<Node>>::maybe_update(&mut (*old_pointer).links, new_value.links) }
    }
}

impl<Node> Default for OriginGraph<Node> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Node> OriginGraph<Node> {
    pub const fn new() -> Self {
        Self { links: Vec::new() }
    }

    pub fn from_links(links: Vec<OriginLink<Node>>) -> Self {
        Self { links }
    }

    pub fn push(&mut self, from: Node, to: Node, kind: OriginLinkKind) {
        self.links.push(OriginLink::new(from, to, kind));
    }

    pub fn push_link(&mut self, link: OriginLink<Node>) {
        self.links.push(link);
    }

    pub fn extend(&mut self, links: impl IntoIterator<Item = OriginLink<Node>>) {
        self.links.extend(links);
    }

    pub fn links(&self) -> &[OriginLink<Node>] {
        &self.links
    }

    pub fn into_links(self) -> Vec<OriginLink<Node>> {
        self.links
    }

    pub fn iter(&self) -> slice::Iter<'_, OriginLink<Node>> {
        self.links.iter()
    }

    pub fn len(&self) -> usize {
        self.links.len()
    }

    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }
}

impl<Node> OriginGraph<Node>
where
    Node: PartialEq,
{
    pub fn outgoing_from<'a>(
        &'a self,
        node: &'a Node,
    ) -> impl Iterator<Item = &'a OriginLink<Node>> + 'a {
        self.links.iter().filter(move |link| link.from() == node)
    }

    pub fn incoming_to<'a>(
        &'a self,
        node: &'a Node,
    ) -> impl Iterator<Item = &'a OriginLink<Node>> + 'a {
        self.links.iter().filter(move |link| link.to() == node)
    }
}
