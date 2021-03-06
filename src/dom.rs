use std::{
    fmt,
    rc::Rc,
    cell::RefCell,
    hash::{Hash, Hasher},
    sync::atomic::{AtomicUsize, Ordering},
    collections::{BTreeMap, BTreeSet},
    iter::FromIterator,
};
use glium::{Texture2d, framebuffer::SimpleFrameBuffer};
use {
    FastHashMap,
    window::{WindowEvent, WindowInfo},
    images::{ImageId, ImageState},
    cache::DomHash,
    text_cache::TextId,
    traits::Layout,
    app_state::AppState,
    id_tree::{NodeId, Node, Arena},
    default_callbacks::{DefaultCallbackId, StackCheckedPointer},
    window::HidpiAdjustedBounds,
    text_layout::{Words, FontMetrics, TextSizePx},
    css_parser::ParsedCssProperty,
};

static TAG_ID: AtomicUsize = AtomicUsize::new(1);

pub(crate) type TagId = u64;

fn new_tag_id() -> TagId {
    TAG_ID.fetch_add(1, Ordering::SeqCst) as TagId
}

/// A callback function has to return if the screen should
/// be updated after the function has run.PartialEq
///
/// This is necessary for updating the screen only if it is absolutely necessary.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum UpdateScreen {
    /// Redraw the screen
    Redraw,
    /// Don't redraw the screen
    DontRedraw,
}

/// This exist so you can conveniently use the `?` and `.into()` for your own code
///
/// - `Some`: `Redraw`
/// - `None`: `DontRedraw`
impl<T> From<Option<T>> for UpdateScreen {
    fn from(input: Option<T>) -> Self {
        match input {
            None => UpdateScreen::DontRedraw,
            Some(_) => UpdateScreen::Redraw,
        }
    }
}

/// This exist so you can conveniently use the `?` and `.into()` for your own code
///
/// - `Ok` -> `Redraw`
/// - `Err` -> `DontRedraw`
impl<T, E> From<Result<T, E>> for UpdateScreen {
    fn from(input: Result<T, E>) -> Self {
        match input {
            Ok(_) => UpdateScreen::DontRedraw,
            Err(_) => UpdateScreen::Redraw,
        }
    }
}

/// Stores a function pointer that is executed when the given UI element is hit
///
/// Must return an `UpdateScreen` that denotes if the screen should be redrawn.
/// The CSS is not affected by this, so if you push to the windows' CSS inside the
/// function, the screen will not be automatically redrawn, unless you return an
/// `UpdateScreen::Redraw` from the function
pub struct Callback<T: Layout>(pub fn(&mut AppState<T>, WindowEvent<T>) -> UpdateScreen);

// #[derive(Debug, Clone, PartialEq, Hash, Eq)] for Callback<T>

impl<T: Layout> fmt::Debug for Callback<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Callback @ 0x{:x}", self.0 as usize)
    }
}

impl<T: Layout> Clone for Callback<T> {
    fn clone(&self) -> Self {
        Callback(self.0.clone())
    }
}

/// As a hashing function, we use the function pointer casted to a usize
/// as a unique ID for the function. This way, we can hash and compare DOM nodes
/// (to create diffs between two states). Comparing usizes is more efficient
/// than re-creating the whole DOM and serves as a caching mechanism.
impl<T: Layout> Hash for Callback<T> {
  fn hash<H>(&self, state: &mut H) where H: Hasher {
    state.write_usize(self.0 as usize);
  }
}

/// Basically compares the function pointers and types for equality
impl<T: Layout> PartialEq for Callback<T> {
  fn eq(&self, rhs: &Self) -> bool {
    self.0 as usize == rhs.0 as usize
  }
}

impl<T: Layout> Eq for Callback<T> { }

impl<T: Layout> Copy for Callback<T> { }


pub struct GlTextureCallback<T: Layout>(pub fn(&StackCheckedPointer<T>, WindowInfo<T>, HidpiAdjustedBounds) -> Option<Texture>);

// #[derive(Debug, Clone, PartialEq, Hash, Eq)] for GlTextureCallback<T>

impl<T: Layout> fmt::Debug for GlTextureCallback<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "GlTextureCallback @ 0x{:x}", self.0 as usize)
    }
}

impl<T: Layout> Clone for GlTextureCallback<T> {
    fn clone(&self) -> Self {
        GlTextureCallback(self.0.clone())
    }
}

impl<T: Layout> Hash for GlTextureCallback<T> {
  fn hash<H>(&self, state: &mut H) where H: Hasher {
    state.write_usize(self.0 as usize);
  }
}

impl<T: Layout> PartialEq for GlTextureCallback<T> {
  fn eq(&self, rhs: &Self) -> bool {
    self.0 as usize == rhs.0 as usize
  }
}

impl<T: Layout> Eq for GlTextureCallback<T> { }
impl<T: Layout> Copy for GlTextureCallback<T> { }

pub struct IFrameCallback<T: Layout>(pub fn(&StackCheckedPointer<T>, WindowInfo<T>, HidpiAdjustedBounds) -> Dom<T>);

// #[derive(Debug, Clone, PartialEq, Hash, Eq)] for IFrameCallback<T>

impl<T: Layout> fmt::Debug for IFrameCallback<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "IFrameCallback @ 0x{:x}", self.0 as usize)
    }
}

impl<T: Layout> Clone for IFrameCallback<T> {
    fn clone(&self) -> Self {
        IFrameCallback(self.0.clone())
    }
}

impl<T: Layout> Hash for IFrameCallback<T> {
  fn hash<H>(&self, state: &mut H) where H: Hasher {
    state.write_usize(self.0 as usize);
  }
}

impl<T: Layout> PartialEq for IFrameCallback<T> {
  fn eq(&self, rhs: &Self) -> bool {
    self.0 as usize == rhs.0 as usize
  }
}

impl<T: Layout> Eq for IFrameCallback<T> { }

impl<T: Layout> Copy for IFrameCallback<T> { }


/// List of core DOM node types built-into by `azul`.
pub enum NodeType<T: Layout> {
    /// Regular div with no particular type of data attached
    Div,
    /// A small label that can be (optionally) be selectable with the mouse
    Label(String),
    /// Larger amount of text, that has to be cached
    Text(TextId),
    /// An image that is rendered by WebRender. The id is acquired by the
    /// `AppState::add_image()` function
    Image(ImageId),
    /// OpenGL texture. The `Svg` widget deserizalizes itself into a texture
    /// Equality and Hash values are only checked by the OpenGl texture ID,
    /// Azul does not check that the contents of two textures are the same
    GlTexture((GlTextureCallback<T>, StackCheckedPointer<T>)),
    /// DOM that gets passed its width / height during the layout
    IFrame((IFrameCallback<T>, StackCheckedPointer<T>)),
}

// #[derive(Debug, Clone, PartialEq, Hash, Eq)] for NodeType<T>

impl<T: Layout> fmt::Debug for NodeType<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::NodeType::*;
        match self {
            Div => write!(f, "NodeType::Div"),
            Label(a) => write!(f, "NodeType::Label {{ {:?} }}", a),
            Text(a) => write!(f, "NodeType::Text {{ {:?} }}", a),
            Image(a) => write!(f, "NodeType::Image {{ {:?} }}", a),
            GlTexture((ptr, cb)) => write!(f, "NodeType::GlTexture {{ ptr: {:?}, callback: {:?} }}", ptr, cb),
            IFrame((ptr, cb)) => write!(f, "NodeType::IFrame {{ ptr: {:?}, callback: {:?} }}", ptr, cb),
        }
    }
}

impl<T: Layout> Clone for NodeType<T> {
    fn clone(&self) -> Self {
        use self::NodeType::*;
        match self {
            Div => Div,
            Label(a) => Label(a.clone()),
            Text(a) => Text(a.clone()),
            Image(a) => Image(a.clone()),
            GlTexture((ptr, a)) => GlTexture((ptr.clone(), a.clone())),
            IFrame((ptr, a)) => IFrame((ptr.clone(), a.clone())),
        }
    }
}

impl<T: Layout> Hash for NodeType<T> {
    fn hash<H>(&self, state: &mut H) where H: Hasher {
        use self::NodeType::*;
        use std::mem;
        mem::discriminant(&self).hash(state);
        match self {
            Div => { },
            Label(a) => a.hash(state),
            Text(a) => a.hash(state),
            Image(a) => a.hash(state),
            GlTexture((ptr, a)) => {
                ptr.hash(state);
                a.hash(state);
            },
            IFrame((ptr, a)) => {
                ptr.hash(state);
                a.hash(state);
            },
        }
    }
}

impl<T: Layout> PartialEq for NodeType<T> {
    fn eq(&self, rhs: &Self) -> bool {
        use self::NodeType::*;
        match (self, rhs) {
            (Div, Div) => true,
            (Label(a), Label(b)) => a == b,
            (Text(a), Text(b)) => a == b,
            (Image(a), Image(b)) => a == b,
            (GlTexture((ptr_a, a)), GlTexture((ptr_b, b))) => {
                a == b && ptr_a == ptr_b
            },
            (IFrame((ptr_a, a)), IFrame((ptr_b, b))) => {
                a == b && ptr_a == ptr_b
            },
            _ => false,
        }
    }
}

impl<T: Layout> Eq for NodeType<T> { }

impl<T: Layout> NodeType<T> {

    pub fn label<S: Into<String>>(value: S) -> Self {
        NodeType::Label(value.into())
    }

    pub(crate) fn get_css_id(&self) -> &'static str {
        use self::NodeType::*;
        match self {
            Div => "div",
            Label(_) | Text(_) => "p",
            Image(_) => "image",
            GlTexture(_) => "texture",
            IFrame(_) => "iframe",
        }
    }

    /// Returns the preferred width, for example for an image, that would be the
    /// original width (an image always wants to take up the original space)
    pub(crate) fn get_preferred_width(&self, image_cache: &FastHashMap<ImageId, ImageState>) -> Option<f32> {
        use self::NodeType::*;
        match self {
            Image(i) => image_cache.get(i).and_then(|image_state| Some(image_state.get_dimensions().0)),
            Label(_) | Text(_) => /* TODO: Calculate the minimum width for the text? */ None,
            _ => None,
        }
    }

    /// Given a certain width, returns the
    pub(crate) fn get_preferred_height_based_on_width(
        &self,
        div_width: TextSizePx,
        image_cache: &FastHashMap<ImageId, ImageState>,
        words: Option<&Words>,
        font_metrics: Option<FontMetrics>,
    ) -> Option<TextSizePx>
    {
        use self::NodeType::*;
        use css_parser::{LayoutOverflow, TextOverflowBehaviour, TextOverflowBehaviourInner};

        match self {
            Image(i) => image_cache.get(i).and_then(|image_state| {
                let (image_original_height, image_original_width) = image_state.get_dimensions();
                Some(div_width * (image_original_width / image_original_height))
            }),
            Label(_) | Text(_) => {
                let (words, font) = (words?, font_metrics?);
                let vertical_info = words.get_vertical_height(&LayoutOverflow {
                    horizontal: TextOverflowBehaviour::Modified(TextOverflowBehaviourInner::Scroll),
                    .. Default::default()
                }, &font, div_width);
                Some(vertical_info.vertical_height)
            }
            _ => None,
        }
    }
}

/// When to call a callback action - `On::MouseOver`, `On::MouseOut`, etc.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum On {
    /// Mouse cursor is hovering over the element
    MouseOver,
    /// Mouse cursor has is over element and is pressed
    /// (not good for "click" events - use `MouseUp` instead)
    MouseDown,
    /// (Specialization of `MouseDown`). Fires only if the left mouse button
    /// has been pressed while cursor was over the element
    LeftMouseDown,
    /// (Specialization of `MouseDown`). Fires only if the middle mouse button
    /// has been pressed while cursor was over the element
    MiddleMouseDown,
    /// (Specialization of `MouseDown`). Fires only if the right mouse button
    /// has been pressed while cursor was over the element
    RightMouseDown,
    /// Mouse button has been released while cursor was over the element
    MouseUp,
    /// (Specialization of `MouseUp`). Fires only if the left mouse button has
    /// been released while cursor was over the element
    LeftMouseUp,
    /// (Specialization of `MouseUp`). Fires only if the middle mouse button has
    /// been released while cursor was over the element
    MiddleMouseUp,
    /// (Specialization of `MouseUp`). Fires only if the right mouse button has
    /// been released while cursor was over the element
    RightMouseUp,
    /// Mouse cursor has entered the element
    MouseEnter,
    /// Mouse cursor has left the element
    MouseLeave,
    /// Mousewheel / touchpad scrolling
    Scroll,
    /// A key was pressed. Check `window.get_keyboard_state().current_chars` for
    /// getting the actual key / virtual key / scancode.
    ///
    /// Warning: key repeat is on. When a key is held down, this event fires
    /// multiple times, the delay between events depends on the operating system.
    KeyDown,
    /// A key was released. Check `window.get_keyboard_state().current_chars` for
    /// getting the actual key / virtual key / scancode
    ///
    /// Warning: key repeat is on. When a key is held down, this event fires
    /// multiple times, the delay between events depends on the operating system.
    KeyUp,
}

pub struct NodeData<T: Layout> {
    /// `div`
    pub node_type: NodeType<T>,
    /// `#main`
    pub id: Option<String>,
    /// `.myclass .otherclass`
    pub classes: Vec<String>,
    /// `onclick` -> `my_button_click_handler`
    pub events: CallbackList<T>,
    /// Usually not set by the user directly - `FakeWindow::add_default_callback`
    /// returns a callback ID, so that we know which default callback(s) are attached
    /// to this node.
    ///
    /// This is only important if this node has any default callbacks.
    pub default_callback_ids: Vec<(On, DefaultCallbackId)>,
    /// Force this DOM node to be part of the hit-testing although it may have no
    /// callbacks attached. Default: `BTreeSet::new()`.
    pub force_enable_hit_test: Vec<On>,
    /// Override certain dynamic styling properties in this frame. For this,
    /// these properties have to have a name (the ID).
    ///
    /// For example, in the CSS stylesheet:
    ///
    /// ```css,ignore
    /// #my_item { width: [[ my_custom_width | 200px ]] }
    /// ```
    ///
    /// ```rust,ignore
    /// let node = NodeData {
    ///     id: Some("my_item".into()),
    ///     dynamic_css_overrides: vec![("my_custom_width".into(), ParsedCssProperty::Width(LayoutWidth::px(500.0)))]
    /// }
    /// ```
    pub dynamic_css_overrides: Vec<(String, ParsedCssProperty)>
}

impl<T: Layout> PartialEq for NodeData<T> {
    fn eq(&self, other: &Self) -> bool {
        self.node_type == other.node_type &&
        self.id == other.id &&
        self.classes == other.classes &&
        self.events == other.events &&
        self.default_callback_ids == other.default_callback_ids &&
        self.force_enable_hit_test == other.force_enable_hit_test &&
        self.dynamic_css_overrides == other.dynamic_css_overrides
    }
}

impl<T: Layout> Eq for NodeData<T> { }

impl<T: Layout> Default for NodeData<T> {
    fn default() -> Self {
        NodeData {
            node_type: NodeType::Div,
            id: None,
            classes: Vec::new(),
            events: CallbackList::default(),
            default_callback_ids: Vec::new(),
            force_enable_hit_test: Vec::new(),
            dynamic_css_overrides: Vec::new(),
        }
    }
}

impl<T: Layout> Hash for NodeData<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.node_type.hash(state);
        self.id.hash(state);
        for class in &self.classes {
            class.hash(state);
        }
        for default_callback_id in &self.default_callback_ids {
            default_callback_id.hash(state);
        }
        self.events.hash(state);
        self.force_enable_hit_test.hash(state);
        for override_property in &self.dynamic_css_overrides {
            override_property.hash(state);
        }
    }
}

impl<T: Layout> Clone for NodeData<T> {
    fn clone(&self) -> Self {
        Self {
            node_type: self.node_type.clone(),
            id: self.id.clone(),
            classes: self.classes.clone(),
            events: self.events.clone(),
            default_callback_ids: self.default_callback_ids.clone(),
            force_enable_hit_test: self.force_enable_hit_test.clone(),
            dynamic_css_overrides: self.dynamic_css_overrides.clone(),
        }
    }
}

impl<T: Layout> fmt::Display for NodeData<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const DEFAULT: &str = "";
        let id = match &self.id {
            Some(s) => s,
            None => DEFAULT,
        };
        write!(f, "[{} #{} .{:?}]", self.node_type.get_css_id(), id, self.classes)
    }
}

impl<T: Layout> fmt::Debug for NodeData<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
            "NodeData {{ \
                \tnode_type: {:?}, \
                \tid: {:?}, \
                \tclasses: {:?}, \
                \tevents: {:?}, \
                \tdefault_callback_ids: {:?}, \
                \tforce_enable_hit_test: {:?}, \
                \tdynamic_css_overrides: {:?}, \
            }}",
        self.node_type,
        self.id,
        self.classes,
        self.events,
        self.default_callback_ids,
        self.force_enable_hit_test,
        self.dynamic_css_overrides)
    }
}

impl<T: Layout> NodeData<T> {

    pub(crate) fn calculate_node_data_hash(&self) -> DomHash {
        use std::hash::Hash;

        // Pick hash algorithm based on features
        #[cfg(feature = "faster-hashing")]
        use twox_hash::XxHash as HashAlgorithm;
        #[cfg(not(feature = "faster-hashing"))]
        use std::collections::hash_map::DefaultHasher as HashAlgorithm;

        let mut hasher = HashAlgorithm::default();
        self.hash(&mut hasher);
        DomHash(hasher.finish())
    }

    /// Creates a new NodeData
    pub fn new(node_type: NodeType<T>) -> Self {
        Self {
            node_type,
            .. Default::default()
        }
    }
}

pub struct CallbackList<T: Layout> {
    pub callbacks: BTreeMap<On, Callback<T>>
}

impl<T: Layout> Default for CallbackList<T> {
    fn default() -> Self {
        Self {
            callbacks: BTreeMap::default(),
        }
    }
}

impl<T: Layout> Hash for CallbackList<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for callback in &self.callbacks {
            callback.hash(state);
        }
    }
}

impl<T: Layout> fmt::Debug for CallbackList<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CallbackList (length: {:?})", self.callbacks.len())
    }
}

impl<T: Layout> Clone for CallbackList<T> {
    fn clone(&self) -> Self {
        CallbackList { callbacks: self.callbacks.clone() }
    }
}

impl<T: Layout> PartialEq for CallbackList<T> {
  fn eq(&self, rhs: &Self) -> bool {
    if self.callbacks.len() != rhs.callbacks.len() {
        return false;
    }
    self.callbacks.iter().all(|(key, val)| {
        rhs.callbacks.get(key) == Some(val)
    })
  }
}

impl<T: Layout> Eq for CallbackList<T> { }

/// The document model, similar to HTML. This is a create-only structure, you don't actually read anything back
#[derive(Clone, PartialEq, Eq)]
pub struct Dom<T: Layout> {
    pub(crate) arena: Rc<RefCell<Arena<NodeData<T>>>>,
    pub(crate) root: NodeId,
    pub(crate) head: NodeId,
}

impl<T: Layout> fmt::Debug for Dom<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
        "Dom {{ arena: {:?}, root: {:?}, head: {:?} }}",
        self.arena,
        self.root,
        self.head)
    }
}

impl<T: Layout> FromIterator<Dom<T>> for Dom<T> {
    fn from_iter<I: IntoIterator<Item=Dom<T>>>(iter: I) -> Self {
        let mut c = Dom::new(NodeType::Div);
        for i in iter {
            c.add_child(i);
        }
        c
    }
}

impl<T: Layout> FromIterator<NodeData<T>> for Dom<T> {
    fn from_iter<I: IntoIterator<Item=NodeData<T>>>(iter: I) -> Self {

        use id_tree::Node;

        // We have to use a "root" node, otherwise we run into problems if
        // the iterator executes 0 times (and therefore pushes 0 nodes)

        // "Root" node of this DOM
        let mut nodes = vec![Node {
            data: NodeData::new(NodeType::Div),
            parent: None,
            previous_sibling: None,
            next_sibling: None,
            last_child: None,
            first_child: None,
        }];

        let mut idx = 0;

        for i in iter {
            let node = Node {
                data: i,
                parent: Some(NodeId::new(0)),
                previous_sibling: if idx == 0 { None } else { Some(NodeId::new(idx)) },
                next_sibling: Some(NodeId::new(idx + 2)),
                last_child: None,
                first_child: None,
            };
            nodes.push(node);
            idx += 1;
        }

        let nodes_len = nodes.len();

        // nodes_len is always at least 1, since we pushed the original root node
        // Check if there is a child DOM
        if nodes_len > 1 {
            if let Some(last) = nodes.get_mut(nodes_len - 1) {
                last.next_sibling = None;
            }
            nodes[0].last_child = Some(NodeId::new(nodes_len - 1));
            nodes[0].first_child = Some(NodeId::new(1));
        }

        Dom { head: NodeId::new(0), root: NodeId::new(0), arena: Rc::new(RefCell::new(Arena { nodes })) }
    }
}

impl<T: Layout> FromIterator<NodeType<T>> for Dom<T> {
    fn from_iter<I: IntoIterator<Item=NodeType<T>>>(iter: I) -> Self {
        iter.into_iter().map(|i| NodeData { node_type: i, .. Default::default() }).collect()
    }
}

impl<T: Layout> Dom<T> {

    /// Creates an empty DOM
    #[inline]
    pub fn new(node_type: NodeType<T>) -> Self {
        Self::with_capacity(node_type, 0)
    }

    /// Returns the number of nodes in this DOM
    #[inline]
    pub fn len(&self) -> usize {
        self.arena.borrow().nodes_len()
    }

    /// Creates an empty DOM with space reserved for `cap` nodes
    #[inline]
    pub fn with_capacity(node_type: NodeType<T>, cap: usize) -> Self {
        let mut arena = Arena::with_capacity(cap.saturating_add(1));
        let root = arena.new_node(NodeData::new(node_type));
        Self {
            arena: Rc::new(RefCell::new(arena)),
            root: root,
            head: root,
        }
    }

    /// Adds a child DOM to the current DOM
    pub fn add_child(&mut self, child: Self) {

        // Note: for a more readable Python version of this algorithm,
        // see: https://gist.github.com/fschutt/4b3bd9a2654b548a6eb0b6a8623bdc8a#file-dow_new_2-py-L65-L107

        let self_len = self.arena.borrow().nodes_len();
        let child_len = child.arena.borrow().nodes_len();

        if child_len == 0 {
            // No nodes to append, nothing to do
            return;
        }

        if self_len == 0 {
            // Self has no nodes, therefore all child nodes will
            // replace the self nodes, so
            *self = child;
            return;
        }

        let mut self_arena = self.arena.borrow_mut();
        let mut child_arena = child.arena.borrow_mut();

        let mut last_sibling = None;

        for node_id in 0..child_len {
            let node_id = NodeId::new(node_id);
            let node: &mut Node<NodeData<T>> = &mut child_arena[node_id];

            // WARNING: Order of these blocks is important!

            if node.previous_sibling_mut().and_then(|previous_sibling| {
                // Some(previous_sibling) - increase the parent ID by the current arena length
                *previous_sibling += self_len;
                Some(previous_sibling)
            }).is_none() {
                // None - set the current heads' last child as the new previous sibling
                let last_child = self_arena[self.head].last_child;
                if last_child.is_some() && node.parent.is_none() {
                    node.previous_sibling = last_child;
                    self_arena[last_child.unwrap()].next_sibling = Some(node_id + self_len);
                }
            }

            if node.parent_mut().and_then(|parent| {
                *parent += self_len;
                Some(parent)
            }).is_none() {
                // Have we encountered the last root item?
                if node.next_sibling.is_none() {
                    last_sibling = Some(node_id);
                }
                node.parent = Some(self.head);
            }

            if let Some(next_sibling) = node.next_sibling_mut() {
                *next_sibling += self_len;
            }

            if let Some(first_child) = node.first_child_mut() {
                *first_child += self_len;
            }

            if let Some(last_child) = node.last_child_mut() {
                *last_child += self_len;
            }
        }

        self_arena[self.head].first_child.get_or_insert(NodeId::new(self_len));
        self_arena[self.head].last_child = Some(last_sibling.unwrap() + self_len);

        (&mut *self_arena).append(&mut child_arena);
    }

    /// Same as `id`, but easier to use for method chaining in a builder-style pattern
    #[inline]
    pub fn with_id<S: Into<String>>(mut self, id: S) -> Self {
        self.set_id(id);
        self
    }

    /// Same as `id`, but easier to use for method chaining in a builder-style pattern
    #[inline]
    pub fn with_class<S: Into<String>>(mut self, class: S) -> Self {
        self.add_class(class);
        self
    }

    /// Same as `event`, but easier to use for method chaining in a builder-style pattern
    #[inline]
    pub fn with_callback(mut self, on: On, callback: Callback<T>) -> Self {
        self.add_callback(on, callback);
        self
    }

    #[inline]
    pub fn with_child(mut self, child: Self) -> Self {
        self.add_child(child);
        self
    }

    /// Enable the DOM to trigger a hit-test on the specified event, although
    /// there may not be any callbacks attached.
    #[inline]
    pub fn with_hit_test(mut self, on: On) -> Self {
        self.enable_hit_testing(on);
        self
    }

    #[inline]
    pub fn with_css_override<S: Into<String>>(mut self, id: S, property: ParsedCssProperty) -> Self {
        self.add_css_override(id, property);
        self
    }

    #[inline]
    pub fn set_id<S: Into<String>>(&mut self, id: S) {
        self.arena.borrow_mut()[self.head].data.id = Some(id.into());
    }

    #[inline]
    pub fn add_class<S: Into<String>>(&mut self, class: S) {
        self.arena.borrow_mut()[self.head].data.classes.push(class.into());
    }

    #[inline]
    pub fn add_callback(&mut self, on: On, callback: Callback<T>) {
        self.arena.borrow_mut()[self.head].data.events.callbacks.insert(on, callback);
    }

    #[inline]
    pub fn add_default_callback_id(&mut self, on: On, id: DefaultCallbackId) {
        self.arena.borrow_mut()[self.head].data.default_callback_ids.push((on, id));
    }

    #[inline]
    pub fn add_css_override<S: Into<String>>(&mut self, override_id: S, property: ParsedCssProperty) {
        self.arena.borrow_mut()[self.head].data.dynamic_css_overrides.push((override_id.into(), property));
    }

    /// Prints a debug formatted version of the DOM for easier debugging
    pub fn debug_dump(&self) {
        println!("{}", self.arena.borrow().print_tree(|t| format!("{}", t)));
    }

    /// Enables the current DOM element to be hit-tested even though it may
    /// not have any callbacks or default callbacks attached to it. See the
    /// two-way data binding tutorial on why this is useful (last section)
    #[inline]
    pub fn enable_hit_testing(&mut self, when_to_hit_test: On) {
        self.arena.borrow_mut()[self.head].data.force_enable_hit_test.push(when_to_hit_test);
    }

    pub(crate) fn collect_callbacks(
        &self,
        tag_ids_to_callback_list: &mut BTreeMap<TagId, BTreeMap<On, Callback<T>>>,
        tag_ids_to_default_callback_list: &mut BTreeMap<TagId, BTreeMap<On, DefaultCallbackId>>,
        tag_ids_to_noop_callbacks: &mut BTreeMap<TagId, BTreeSet<On>>,
        node_ids_to_tag_ids: &mut BTreeMap<NodeId, TagId>,
        tag_ids_to_node_ids: &mut BTreeMap<TagId, NodeId>,
        dynamic_css_overrides: &mut BTreeMap<NodeId, FastHashMap<String, ParsedCssProperty>>)
    {
        let arena = self.arena.borrow();

        for node_id in arena.linear_iter() {

            let item = &arena[node_id];

            let mut node_tag_id = None;

            if !item.data.events.callbacks.is_empty() {
                let tag_id = new_tag_id();
                tag_ids_to_callback_list.insert(tag_id, item.data.events.callbacks.clone());
                node_tag_id = Some(tag_id);
            }

            if !item.data.default_callback_ids.is_empty() {
                let tag_id = node_tag_id.unwrap_or(new_tag_id());
                tag_ids_to_default_callback_list.insert(tag_id, item.data.default_callback_ids.iter().cloned().collect());
                node_tag_id = Some(tag_id);
            }

            // Force-enabling hit-testing is important for child nodes that don't have any
            // callbacks attached themselves, but their parents need them to be hit-tested
            if !item.data.force_enable_hit_test.is_empty() {
                let tag_id = node_tag_id.unwrap_or(new_tag_id());
                tag_ids_to_noop_callbacks.insert(tag_id, item.data.force_enable_hit_test.iter().cloned().collect());
                node_tag_id = Some(tag_id);
            }

            if let Some(tag_id) = node_tag_id {
                tag_ids_to_node_ids.insert(tag_id, node_id);
                node_ids_to_tag_ids.insert(node_id, tag_id);
            }

            // Collect all the styling overrides into one hash map
            if !item.data.dynamic_css_overrides.is_empty() {
                dynamic_css_overrides.insert(node_id, item.data.dynamic_css_overrides.iter().cloned().collect());
            }
        }

        TAG_ID.swap(1, Ordering::SeqCst);
    }
}

/// OpenGL texture, use `ReadOnlyWindow::create_texture` to create a texture
///
/// **WARNING**: Don't forget to call `ReadOnlyWindow::unbind_framebuffer()`
/// when you are done with your OpenGL drawing, otherwise WebRender will render
/// to the texture, not the window, so your texture will actually never show up.
/// If you use a `Texture` and you get a blank screen, this is probably why.
#[derive(Debug, Clone)]
pub struct Texture {
    pub(crate) inner: Rc<Texture2d>,
}

impl Texture {
    pub(crate) fn new(tex: Texture2d) -> Self {
        Self {
            inner: Rc::new(tex),
        }
    }

    /// Prepares the texture for drawing - you can only draw
    /// on a framebuffer, the texture itself is readonly from the
    /// OpenGL drivers point of view.
    ///
    /// **WARNING**: Don't forget to call `ReadOnlyWindow::unbind_framebuffer()`
    /// when you are done with your OpenGL drawing, otherwise WebRender will render
    /// to the texture instead of the window, so your texture will actually
    /// never show up on the screen, since it is never rendered.
    /// If you use a `Texture` and you get a blank screen, this is probably why.
    pub fn as_surface<'a>(&'a self) -> SimpleFrameBuffer<'a> {
        self.inner.as_surface()
    }
}

impl Hash for Texture {
    fn hash<H: Hasher>(&self, state: &mut H) {
        use glium::GlObject;
        self.inner.get_id().hash(state);
    }
}

impl PartialEq for Texture {
    /// Note: Comparison uses only the OpenGL ID, it doesn't compare the
    /// actual contents of the texture.
    fn eq(&self, other: &Texture) -> bool {
        use glium::GlObject;
        self.inner.get_id() == other.inner.get_id()
    }
}

impl Eq for Texture { }

#[test]
fn test_dom_sibling_1() {

    struct TestLayout { }

    impl Layout for TestLayout {
        fn layout(&self) -> Dom<Self> {
            Dom::new(NodeType::Div)
                .with_child(
                    Dom::new(NodeType::Div)
                    .with_id("sibling-1")
                    .with_child(Dom::new(NodeType::Div)
                        .with_id("sibling-1-child-1")))
                .with_child(Dom::new(NodeType::Div)
                    .with_id("sibling-2")
                    .with_child(Dom::new(NodeType::Div)
                        .with_id("sibling-2-child-1")))
        }
    }

    let dom = TestLayout{ }.layout();
    let arena = dom.arena.borrow();

    assert_eq!(NodeId::new(0), dom.root);

    assert_eq!(Some(String::from("sibling-1")),
        arena[
            arena[dom.root]
            .first_child().expect("root has no first child")
        ].data.id);

    assert_eq!(Some(String::from("sibling-2")),
        arena[
            arena[
                arena[dom.root]
                .first_child().expect("root has no first child")
            ].next_sibling().expect("root has no second sibling")
        ].data.id);

    assert_eq!(Some(String::from("sibling-1-child-1")),
        arena[
            arena[
                arena[dom.root]
                .first_child().expect("root has no first child")
            ].first_child().expect("first child has no first child")
        ].data.id);

    assert_eq!(Some(String::from("sibling-2-child-1")),
        arena[
            arena[
                arena[
                    arena[dom.root]
                    .first_child().expect("root has no first child")
                ].next_sibling().expect("first child has no second sibling")
            ].first_child().expect("second sibling has no first child")
        ].data.id);
}

#[test]
fn test_dom_from_iter_1() {

    use id_tree::Node;

    struct TestLayout { }

    impl Layout for TestLayout {
        fn layout(&self) -> Dom<Self> {
            (0..5).map(|e| NodeData::new(NodeType::Label(format!("{}", e + 1)))).collect()
        }
    }

    let dom = TestLayout{ }.layout();
    let arena = dom.arena.borrow();

    // We need to have 6 nodes:
    //
    // root                 NodeId(0)
    //   |-> 1              NodeId(1)
    //   |-> 2              NodeId(2)
    //   |-> 3              NodeId(3)
    //   |-> 4              NodeId(4)
    //   '-> 5              NodeId(5)

    assert_eq!(arena.nodes_len(), 6);

    // Check root node
    assert_eq!(arena.nodes.first(), Some(&Node {
        parent: None,
        previous_sibling: None,
        next_sibling: None,
        first_child: Some(NodeId::new(1)),
        last_child: Some(NodeId::new(5)),
        data: NodeData::new(NodeType::Div),
    }));

    assert_eq!(arena.nodes.last(), Some(&Node {
        parent: Some(NodeId::new(0)),
        previous_sibling: Some(NodeId::new(4)),
        next_sibling: None,
        first_child: None,
        last_child: None,
        data: NodeData {
            node_type: NodeType::Label(String::from("5")),
            .. Default::default()
        }
    }));
}

/// Test that there shouldn't be a DOM that has 0 nodes
#[test]
fn test_zero_size_dom() {

    struct TestLayout { }

    impl Layout for TestLayout {
        fn layout(&self) -> Dom<Self> {
            Dom::new(NodeType::Div)
        }
    }

    let mut null_dom =
        (0..0)
        .map(|_| NodeData { node_type: NodeType::Div, .. Default::default() })
        .collect::<Dom<TestLayout>>();

    assert!(null_dom.arena.borrow().nodes_len() == 1);

    null_dom.add_class("hello"); // should not panic
    null_dom.set_id("id-hello"); // should not panic
}
