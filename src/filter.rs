//! () 为空过滤器
//! 2种原型过滤器 Without<C> With<C>
//! Or只支持多个With<C>，表示原型上只要有任何1个C就可以
//! Added Changed为迭代器，多个迭代器是或关系， 原型上只要有1个可迭代的组件就可以
//! Query上提供IsChanged
//! Query<(&T, IsChanged<C8>), (Without<C1>,With<C2>,With<C3>,Or<(With<C4>, With<C5>)>, Changed<C6>, Added<C7>)>
//!

use pi_null::Null;
use pi_proc_macros::all_tuples;
use std::any::TypeId;
use std::marker::PhantomData;

use crate::archetype::Archetype;
use crate::system::ReadWrite;
use crate::world::*;

pub trait FilterArchetype {
    fn filter_archetype(_archetype: &Archetype) -> bool {
        false
    }
}
pub trait FilterComponents {
    /// initializes ReadWrite for this [`FilterComponents`] type.
    fn init_read_write(_world: &World, _rw: &mut ReadWrite) {}
    fn archetype_filter(_archetype: &Archetype) -> bool {
        false
    }
}

pub struct Without<T: 'static>(PhantomData<T>);
impl<T: 'static> FilterComponents for Without<T> {
    fn init_read_write(_world: &World, rw: &mut ReadWrite) {
        rw.withouts.insert(TypeId::of::<T>());
    }
    fn archetype_filter(archetype: &Archetype) -> bool {
        !archetype
            .get_mem_offset_ti_index(&TypeId::of::<T>())
            .0
            .is_null()
    }
}

pub struct With<T: 'static>(PhantomData<T>);
impl<T: 'static> FilterArchetype for With<T> {
    fn filter_archetype(archetype: &Archetype) -> bool {
        Self::archetype_filter(archetype)
    }
}
impl<T: 'static> FilterComponents for With<T> {
    fn archetype_filter(archetype: &Archetype) -> bool {
        archetype
            .get_mem_offset_ti_index(&TypeId::of::<T>())
            .0
            .is_null()
    }
}

pub struct Added<T: 'static>(PhantomData<T>);
impl<T: 'static> FilterComponents for Added<T> {
    fn init_read_write(_world: &World, rw: &mut ReadWrite) {
        rw.listeners.push((TypeId::of::<T>(), false));
    }
}

pub struct Changed<T: 'static>(PhantomData<T>);
impl<T: 'static> FilterComponents for Changed<T> {
    fn init_read_write(_world: &World, rw: &mut ReadWrite) {
        rw.listeners.push((TypeId::of::<T>(), true));
    }
}

macro_rules! impl_tuple_fetch {
    ($(($name: ident, $state: ident)),*) => {
        #[allow(non_snake_case)]
        #[allow(clippy::unused_unit)]
        // SAFETY: defers to soundness `$name: FilterComponents` impl
        impl<$($name: FilterComponents),*> FilterComponents for ($($name,)*) {

            fn init_read_write(_world: &World, _rw: &mut ReadWrite) {
                ($($name::init_read_write(_world, _rw),)*);
            }
            fn archetype_filter(_archetype: &Archetype) -> bool {
                ($(
                    if $name::archetype_filter(_archetype){return true},
                )*);
                false
            }
        }

    };
}
all_tuples!(impl_tuple_fetch, 0, 15, F, S);

pub struct Or<T: 'static + FilterArchetype>(PhantomData<T>);
impl<T: 'static + FilterArchetype> FilterComponents for Or<T> {
    fn archetype_filter(archetype: &Archetype) -> bool {
        T::filter_archetype(archetype)
    }
}

macro_rules! impl_or_tuple_fetch {
    ($(($name: ident, $state: ident)),*) => {
        #[allow(non_snake_case)]
        #[allow(clippy::unused_unit)]
        impl<$($name: FilterArchetype),*> FilterArchetype for ($($name,)*) {

            fn filter_archetype(_archetype: &Archetype) -> bool {
                ($(
                    if !$name::filter_archetype(_archetype){return false},
                )*);
                true
            }
        }

    };
}
all_tuples!(impl_or_tuple_fetch, 0, 15, F, S);
