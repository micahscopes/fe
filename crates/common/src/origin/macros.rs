#[macro_export]
macro_rules! define_closed_string_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $value:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )+
        }

        impl $name {
            pub const STRINGS: &'static [&'static str] = &[$($value),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value,)+
                }
            }

            pub fn from_str(raw: &str) -> Option<Self> {
                match raw {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let raw = <::std::string::String as ::serde::Deserialize>::deserialize(
                    deserializer,
                )?;
                Self::from_str(&raw).ok_or_else(|| {
                    <D::Error as ::serde::de::Error>::unknown_variant(&raw, Self::STRINGS)
                })
            }
        }
    };
}

#[macro_export]
macro_rules! define_origin_owner_key {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, $crate::salsa::Update)]
        $vis struct $name(::std::string::String);

        impl $name {
            pub fn new(key: impl Into<::std::string::String>) -> Self {
                Self::try_new(key).unwrap_or_else(|err| panic!("{err}"))
            }

            pub fn try_new(
                key: impl Into<::std::string::String>
            ) -> ::std::result::Result<Self, $crate::origin::OriginKeyTextError> {
                let key = key.into();
                $crate::origin::validate_origin_key_text("origin owner key", &key)?;
                Ok(Self(key))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl $crate::origin::OriginExportOwnerKey for $name {
            fn as_str(&self) -> &str {
                self.as_str()
            }
        }
    };
}

#[macro_export]
macro_rules! define_origin_local_key {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, $crate::salsa::Update)]
        $vis struct $name(::std::string::String);

        impl $name {
            pub fn new(key: impl Into<::std::string::String>) -> Self {
                Self::try_new(key).unwrap_or_else(|err| panic!("{err}"))
            }

            pub fn try_new(
                key: impl Into<::std::string::String>
            ) -> ::std::result::Result<Self, $crate::origin::OriginKeyTextError> {
                let key = key.into();
                $crate::origin::validate_origin_key_text("origin local key", &key)?;
                Ok(Self(key))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl $crate::origin::OriginExportLocalKey for $name {
            fn to_export_local_key(&self) -> ::std::string::String {
                self.as_str().to_string()
            }
        }
    };
}

#[macro_export]
macro_rules! define_origin_string_key {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, $crate::salsa::Update)]
        $vis struct $name(::std::string::String);

        impl $name {
            pub fn new(key: impl Into<::std::string::String>) -> Self {
                Self::try_new(key).unwrap_or_else(|err| panic!("{err}"))
            }

            pub fn try_new(
                key: impl Into<::std::string::String>
            ) -> ::std::result::Result<Self, $crate::origin::OriginKeyTextError> {
                let key = key.into();
                $crate::origin::validate_origin_key_text("origin string key", &key)?;
                Ok(Self(key))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

#[macro_export]
macro_rules! define_origin_key_type {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident $(<$lt:lifetime>)? {
            owner: $owner_ty:ty => $owner:ident,
            local: $local_ty:ty => $local:ident
        }
    ) => {
        $(#[$meta])*
        $vis struct $name $(<$lt>)? {
            key: $crate::origin::OriginKey<$owner_ty, $local_ty>,
        }

        impl $(<$lt>)? $name $(<$lt>)? {
            pub const fn new($owner: $owner_ty, $local: $local_ty) -> Self {
                Self {
                    key: $crate::origin::OriginKey::new($owner, $local),
                }
            }

            pub fn $owner(self) -> $owner_ty {
                self.key.into_parts().0
            }

            pub fn $local(self) -> $local_ty {
                self.key.into_parts().1
            }
        }
    };
}

#[macro_export]
macro_rules! define_origin_graph_type {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident $(<$lt:lifetime>)? ($node:ty);
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        $vis struct $name $(<$lt>)?($crate::origin::OriginGraph<$node>);

        impl $(<$lt>)? ::std::default::Default for $name $(<$lt>)? {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $(<$lt>)? $name $(<$lt>)? {
            pub const fn new() -> Self {
                Self($crate::origin::OriginGraph::new())
            }

            pub fn from_links(
                links: ::std::vec::Vec<$crate::origin::OriginLink<$node>>,
            ) -> Self {
                Self($crate::origin::OriginGraph::from_links(links))
            }

            pub fn push(
                &mut self,
                from: $node,
                to: $node,
                kind: $crate::origin::OriginLinkKind,
            ) {
                self.0.push(from, to, kind);
            }

            pub fn push_link(&mut self, link: $crate::origin::OriginLink<$node>) {
                self.0.push_link(link);
            }

            pub fn extend(
                &mut self,
                links: impl ::std::iter::IntoIterator<
                    Item = $crate::origin::OriginLink<$node>,
                >,
            ) {
                self.0.extend(links);
            }

            pub fn links(&self) -> &[$crate::origin::OriginLink<$node>] {
                self.0.links()
            }

            pub fn into_links(self) -> ::std::vec::Vec<$crate::origin::OriginLink<$node>> {
                self.0.into_links()
            }

            pub fn iter(
                &self,
            ) -> ::std::slice::Iter<'_, $crate::origin::OriginLink<$node>> {
                self.0.iter()
            }

            pub fn len(&self) -> usize {
                self.0.len()
            }

            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }

            pub fn as_origin_graph(&self) -> &$crate::origin::OriginGraph<$node> {
                &self.0
            }

            pub fn into_origin_graph(self) -> $crate::origin::OriginGraph<$node> {
                self.0
            }
        }

        impl $(<$lt>)? $name $(<$lt>)?
        where
            $node: ::std::cmp::PartialEq,
        {
            pub fn outgoing_from<'a>(
                &'a self,
                node: &'a $node,
            ) -> impl ::std::iter::Iterator<
                Item = &'a $crate::origin::OriginLink<$node>,
            > + 'a {
                self.0.outgoing_from(node)
            }

            pub fn incoming_to<'a>(
                &'a self,
                node: &'a $node,
            ) -> impl ::std::iter::Iterator<
                Item = &'a $crate::origin::OriginLink<$node>,
            > + 'a {
                self.0.incoming_to(node)
            }
        }
    };
}
