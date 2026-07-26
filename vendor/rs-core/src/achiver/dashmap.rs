use std::borrow::Borrow;
use std::hash::{BuildHasher, Hash};
use dashmap::DashMap;
use rkyv::{Archive, Deserialize, Fallible, out_field, Serialize};
use rkyv::ser::ScratchSpace;
use rkyv::vec::{ArchivedVec, VecResolver};
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::ser::Serializer;
use dashmap::mapref::multiple::RefMulti;

pub struct DashMapArchiver;

impl<K, V, H> ArchiveWith<DashMap<K, V, H>> for DashMapArchiver
    where
        K::Archived: Eq + Hash,
        K: Archive + Eq + Hash,
        V: Archive,
        H: BuildHasher + Clone,
{
    type Archived = (ArchivedVec<K::Archived>, ArchivedVec<V::Archived>);

    type Resolver = (VecResolver, VecResolver);

    unsafe fn resolve_with(
        field: &DashMap<K, V, H>,
        pos: usize,
        resolver: Self::Resolver,
        out: *mut Self::Archived,
    ) {
        let (off_a, a) = out_field!(out.0);
        ArchivedVec::resolve_from_len(field.len(), pos + off_a, resolver.0, a);
        let (off_a, a) = out_field!(out.1);
        ArchivedVec::resolve_from_len(field.len(), pos + off_a, resolver.1, a);
    }
}

impl<K, V, S, H> SerializeWith<DashMap<K, V, H>, S> for DashMapArchiver
    where
        K::Archived: Eq + Hash,
        K: Archive + Eq + Hash + Serialize<S>,
        V: Archive + Serialize<S>,
        S: ScratchSpace + Serializer + ?Sized,
        H: BuildHasher + Clone,
{
    fn serialize_with(
        field: &DashMap<K, V, H>,
        serializer: &mut S,
    ) -> Result<Self::Resolver, <S as Fallible>::Error> {
        struct CustomSizeHint<I>(I, usize);

        impl<I: Iterator> Iterator for CustomSizeHint<I> {
            type Item = I::Item;

            fn next(&mut self) -> Option<Self::Item> {
                self.0.next()
            }
        }

        impl<I: Iterator> ExactSizeIterator for CustomSizeHint<I> {
            fn len(&self) -> usize {
                self.1
            }
        }

        struct KeyBorrow<'a, K, V, S>(RefMulti<'a, K, V, S>);

        impl<'a, K: Hash + Eq, V, S: BuildHasher> Borrow<K> for KeyBorrow<'a, K, V, S> {
            fn borrow(&self) -> &K {
                self.0.key()
            }
        }
        struct ValueBorrow<'a, K, V, S>(RefMulti<'a, K, V, S>);

        impl<'a, K: Hash + Eq, V, S: BuildHasher> Borrow<V> for ValueBorrow<'a, K, V, S> {
            fn borrow(&self) -> &V {
                self.0.value()
            }
        }

        Ok((
            ArchivedVec::serialize_from_iter::<K, _, _, _>(
                CustomSizeHint(field.iter().map(|entry| KeyBorrow(entry)), field.len()),
                serializer,
            )?,
            ArchivedVec::serialize_from_iter::<V, _, _, _>(
                CustomSizeHint(field.iter().map(|entry| ValueBorrow(entry)), field.len()),
                serializer,
            )?,
        ))
    }
}

impl<K, V, D, H>
DeserializeWith<(ArchivedVec<K::Archived>, ArchivedVec<V::Archived>), DashMap<K, V, H>, D>
for DashMapArchiver
    where
        K::Archived: Eq + Hash + Deserialize<K, D>,
        K: Archive + Eq + Hash,
        V::Archived: Deserialize<V, D>,
        V: Archive,
        D: Fallible + ?Sized,
        H: BuildHasher + Default + Clone,
{
    fn deserialize_with(
        field: &(ArchivedVec<K::Archived>, ArchivedVec<V::Archived>),
        deserializer: &mut D,
    ) -> Result<DashMap<K, V, H>, <D as Fallible>::Error> {
        let map = DashMap::<K, V, H>::with_capacity_and_hasher(field.0.len(), H::default());
        for (k, v) in field.0.iter().zip(field.1.iter()) {
            map.insert(k.deserialize(deserializer)?, v.deserialize(deserializer)?);
        }

        Ok(map)
    }
}