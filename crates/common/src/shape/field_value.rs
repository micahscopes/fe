use std::fmt::Write as _;

pub trait ShapeFieldValue {
    fn shape_field_value(&self) -> String;
}

impl<T: ShapeFieldValue + ?Sized> ShapeFieldValue for &T {
    fn shape_field_value(&self) -> String {
        (*self).shape_field_value()
    }
}

macro_rules! impl_display_shape_field_value {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ShapeFieldValue for $ty {
                fn shape_field_value(&self) -> String {
                    self.to_string()
                }
            }
        )*
    };
}

impl_display_shape_field_value!(
    bool, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

impl ShapeFieldValue for str {
    fn shape_field_value(&self) -> String {
        self.to_owned()
    }
}

impl ShapeFieldValue for String {
    fn shape_field_value(&self) -> String {
        self.clone()
    }
}

impl ShapeFieldValue for [u8] {
    fn shape_field_value(&self) -> String {
        bytes_to_hex(self)
    }
}

impl ShapeFieldValue for Vec<u8> {
    fn shape_field_value(&self) -> String {
        bytes_to_hex(self)
    }
}

impl ShapeFieldValue for Box<[u8]> {
    fn shape_field_value(&self) -> String {
        bytes_to_hex(self)
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02x}").expect("writing to String should not fail");
    }
    out
}
