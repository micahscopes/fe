use super::{ShapeDimension, ShapeFieldValue, ShapeGraph, ShapeGraphHashes, ShapeNodeId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeBuilder {
    graph: ShapeGraph,
    next_auto_key: u32,
}

impl Default for ShapeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeBuilder {
    pub const fn new() -> Self {
        Self {
            graph: ShapeGraph::new(),
            next_auto_key: 0,
        }
    }

    pub fn into_graph(self) -> ShapeGraph {
        self.graph
    }

    pub fn add_described_node(
        &mut self,
        kind: impl Into<String>,
        stable_key: Option<String>,
    ) -> ShapeNodeId {
        let kind = kind.into();
        let stable_key = stable_key.unwrap_or_else(|| {
            let key = format!("auto:{:08}:{kind}", self.next_auto_key);
            self.next_auto_key += 1;
            key
        });
        self.graph.add_node(stable_key, kind)
    }

    pub fn add_field_value<V: ShapeFieldValue + ?Sized>(
        &mut self,
        node: ShapeNodeId,
        dimension: ShapeDimension,
        name: impl Into<String>,
        value: &V,
    ) {
        self.graph
            .add_field(node, dimension, name, value.shape_field_value());
    }

    pub fn add_child_node<T: ShapeDescribe + ?Sized>(
        &mut self,
        parent: ShapeNodeId,
        label: impl Into<String>,
        child: &T,
    ) -> ShapeNodeId {
        let child_node = child.describe_shape(self);
        self.graph.add_child(parent, label, child_node);
        child_node
    }
}

pub trait ShapeDescribe {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId;

    fn shape_graph(&self) -> ShapeGraph
    where
        Self: Sized,
    {
        let mut builder = ShapeBuilder::new();
        self.describe_shape(&mut builder);
        builder.into_graph()
    }

    fn shape_hashes(&self) -> ShapeGraphHashes
    where
        Self: Sized,
    {
        self.shape_graph().hashes()
    }
}

impl<T: ShapeDescribe + ?Sized> ShapeDescribe for Box<T> {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        (**self).describe_shape(builder)
    }
}

impl<T: ShapeDescribe> ShapeDescribe for Option<T> {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        let node = builder.add_described_node("Option", None);
        match self {
            Some(value) => {
                builder.add_field_value(node, ShapeDimension::Structure, "variant", "Some");
                builder.add_child_node(node, "some", value);
            }
            None => builder.add_field_value(node, ShapeDimension::Structure, "variant", "None"),
        }
        node
    }
}

impl<T: ShapeDescribe> ShapeDescribe for Vec<T> {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        self.as_slice().describe_shape(builder)
    }
}

impl<T: ShapeDescribe> ShapeDescribe for [T] {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        let node = builder.add_described_node("slice", None);
        for (idx, child) in self.iter().enumerate() {
            builder.add_child_node(node, format!("item:{idx}"), child);
        }
        node
    }
}

impl<A: ShapeDescribe, B: ShapeDescribe> ShapeDescribe for (A, B) {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        let node = builder.add_described_node("tuple2", None);
        builder.add_child_node(node, "0", &self.0);
        builder.add_child_node(node, "1", &self.1);
        node
    }
}

macro_rules! impl_scalar_shape_describe {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ShapeDescribe for $ty {
                fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
                    let node = builder.add_described_node(stringify!($ty), None);
                    builder.add_field_value(node, ShapeDimension::Constants, "value", self);
                    node
                }
            }
        )*
    };
}

impl_scalar_shape_describe!(
    bool, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);
