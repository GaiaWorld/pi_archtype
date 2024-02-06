
use pi_null::Null;
use pi_proc_macros::all_tuples;
use std::any::TypeId;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use crate::archetype::{Row, Archetype, ColumnIndex};
use crate::column::Column;
use crate::record::ComponentRecord;
use crate::system::ReadWrite;
use crate::table::Table;
use crate::world::*;

pub trait FetchComponents {
    /// The item returned by this [`FetchComponents`]
    type Item<'a>;

    /// Per archetype/table state used by this [`FetchComponents`] to fetch [`Self::Item`](crate::query::FetchComponents::Item)
    type Fetch<'a>: Clone;

    /// State used to construct a [`Self::Fetch`](crate::query::FetchComponents::Fetch). This will be cached inside [`QueryState`](crate::query::QueryState),
    /// so it is best to move as much data / computation here as possible to reduce the cost of
    /// constructing [`Self::Fetch`](crate::query::FetchComponents::Fetch).
    type State: Send + Sync + Sized;

    /// initializes ReadWrite for this [`FetchComponents`] type.
    fn init_read_write(_world: &World, _rw: &mut ReadWrite) {}

    fn archetype_filter(_archetype: &Archetype) -> bool {
        false
    }

    /// Creates and initializes a [`State`](FetchComponents::State) for this [`FetchComponents`] type.
    fn init_state(world: &World, archetype: &Archetype) -> Self::State;

    /// Creates a new instance of this fetch.
    ///
    /// # Safety
    ///
    /// - `world` must have permission to access any of the components specified in `Self::update_archetype_component_access`.
    /// - `state` must have been initialized (via [`FetchComponents::init_state`]) using the same `world` passed
    ///   in to this function.
    fn init_fetch<'w>(
        world: &'w World,
        archetype: &'w Archetype,
        state: &'w Self::State,
        tick: Tick,
    ) -> Self::Fetch<'w>;


    /// Fetch [`Self::Item`](`FetchComponents::Item`) for either the given `entity` in the current [`Table`],
    /// or for the given `entity` in the current [`Archetype`]. This must always be called after
    /// [`FetchComponents::set_table`] with a `table_row` in the range of the current [`Table`] or after
    /// [`FetchComponents::set_archetype`]  with a `entity` in the current archetype.
    ///
    /// # Safety
    ///
    /// Must always be called _after_ [`FetchComponents::set_table`] or [`FetchComponents::set_archetype`]. `entity` and
    /// `table_row` must be in the range of the current table and archetype.
    ///
    /// If `update_component_access` includes any mutable accesses, then the caller must ensure
    /// that `fetch` is called no more than once for each `entity`/`table_row` in each archetype.
    /// If `Self` implements [`ReadOnlyFetchComponents`], then this can safely be called multiple times.
    fn fetch<'w>(
        fetch: &mut Self::Fetch<'w>,
        state: &'w Self::State,
        row: Row,
        e: Entity,
    ) -> Self::Item<'w>;
}

impl FetchComponents for Entity {
    type Fetch<'w> = &'w Table;
    type Item<'w> = Entity;
    type State = ();

    fn init_state(_world: &World, _archetype: &Archetype) -> Self::State {
    }

    fn init_fetch<'w>(_world: &'w World, archetype: &'w Archetype, _state: &'w Self::State, _tick: Tick) -> Self::Fetch<'w> {
        &archetype.table
    }

    #[inline(always)]
    fn fetch<'w>(
        fetch: &mut Self::Fetch<'w>,
        _state: &'w Self::State,
        row: Row,
        e: Entity,
    ) -> Self::Item<'w> {
        e
    }
}

impl<T: 'static> FetchComponents for &T {
    type Fetch<'w> = &'w Column;
    type Item<'w> = &'w T;
    type State = ColumnIndex;

    fn init_read_write(_world: &World, rw: &mut ReadWrite) {
        rw.reads.insert(TypeId::of::<T>());
    }
    fn archetype_filter(archetype: &Archetype) -> bool {
        archetype.get_column(&TypeId::of::<T>()).is_none()
    }
    fn init_state(_world: &World, archetype: &Archetype) -> Self::State {
        archetype.get_column_index(&TypeId::of::<T>())
    }

    #[inline]
    fn init_fetch<'w>(_world: &'w World, archetype: &'w Archetype, state: &'w Self::State, _tick: Tick) -> Self::Fetch<'w> {
        &archetype.table.get_column_unchecked(*state)
    }

    #[inline(always)]
    fn fetch<'w>(
        fetch: &mut Self::Fetch<'w>,
        _state: &'w Self::State,
        row: Row,
        _e: Entity,
    ) -> Self::Item<'w> {
        fetch.get(row)
    }
}

impl<T: 'static> FetchComponents for &mut T {
    type Fetch<'w> = &'w Column;
    // type Fetch<'w> = Option<&'w ComponentRecord>;
    type Item<'w> = Mut<'w, T>;
    type State = ColumnIndex;

    fn init_read_write(_world: &World, rw: &mut ReadWrite) {
        rw.writes.insert(TypeId::of::<T>());
    }
    fn archetype_filter(archetype: &Archetype) -> bool {
        archetype.get_column(&TypeId::of::<T>()).is_none()
    }
    fn init_state(_world: &World, archetype: &Archetype) -> Self::State {
        archetype.get_column_index(&TypeId::of::<T>())
    }

    #[inline]
    fn init_fetch<'w>(_world: &'w World, archetype: &'w Archetype, state: &'w Self::State, _tick: Tick) -> Self::Fetch<'w> {
        &archetype.table.get_column_unchecked(*state)
        // if state.is_null() {
        //     return None;
        // }
        // let r = archetype.get_component_record(state.1);
        // if r.changeds.len() > 0 { Some(r) } else {None}
    }

    #[inline(always)]
    fn fetch<'w>(
        fetch: &mut Self::Fetch<'w>,
        _state: &'w Self::State,
        row: Row,
        _e: Entity,
    ) -> Self::Item<'w> {
        Mut {
            value: fetch.get_mut(row),
            row,
            record: &fetch.record,
        }
    }
}

impl<T: 'static> FetchComponents for Option<&T> {
    type Fetch<'w> = Option<&'w Column>;
    type Item<'w> = Option<&'w T>;
    type State = ColumnIndex;

    fn init_read_write(_world: &World, rw: &mut ReadWrite) {
        rw.reads.insert(TypeId::of::<T>());
    }
    fn init_state(_world: &World, archetype: &Archetype) -> Self::State {
        archetype.get_column_index(&TypeId::of::<T>())
    }

    #[inline]
    fn init_fetch<'w>(_world: &'w World, archetype: &'w Archetype,  state: &'w Self::State,_tick: Tick) -> Self::Fetch<'w> {
        (!state.is_null()).then_some(&archetype.table.get_column_unchecked(*state))
    }

    #[inline(always)]
    fn fetch<'w>(
        fetch: &mut Self::Fetch<'w>,
        _state: &'w Self::State,
        row: Row,
        _e: Entity,
    ) -> Self::Item<'w> {
        fetch.and_then(|c| Some(c.get(row)))
    }
}

impl<T: 'static> FetchComponents for Option<&mut T> {
    type Fetch<'w> = Option<&'w Column>;
    type Item<'w> = Option<Mut<'w, T>>;
    type State = ColumnIndex;

    fn init_read_write(_world: &World, rw: &mut ReadWrite) {
        rw.writes.insert(TypeId::of::<T>());
    }
    fn init_state(_world: &World, archetype: &Archetype) -> Self::State {
        archetype.get_column_index(&TypeId::of::<T>())
    }

    #[inline]
    fn init_fetch<'w>(_world: &'w World, archetype: &'w Archetype, state: &'w Self::State, _tick: Tick) -> Self::Fetch<'w> {
        (!state.is_null()).then_some(&archetype.table.get_column_unchecked(*state))
        // let r = archetype.get_component_record(state.1);
        // if r.changeds.len() > 0 { Some(r) } else {None}
    }

    #[inline(always)]
    fn fetch<'w>(
        fetch: &mut Self::Fetch<'w>,
        _state: &'w Self::State,
        row: Row,
        _e: Entity,
    ) -> Self::Item<'w> {
        fetch.and_then(|c| Some(Mut {
            value: c.get_mut(row),
            row,
            record: &c.record,
        }))
    }
}

pub struct Has<T: 'static> (PhantomData<T>);
impl<T: 'static> FetchComponents for Has<T> {
    type Fetch<'w> = bool;
    type Item<'w> = bool;
    type State = bool;

    fn init_state(_world: &World, archetype: &Archetype) -> Self::State {
        archetype.get_column(&TypeId::of::<T>()).is_some()
    }

    #[inline]
    fn init_fetch<'w>(_world: &'w World, _archetype: &'w Archetype, state: &'w Self::State, _tick: Tick) -> Self::Fetch<'w> {
        *state
    }

    #[inline(always)]
    fn fetch<'w>(
        fetch: &mut Self::Fetch<'w>,
        _state: &'w Self::State,
        _row: Row,
        _e: Entity,
    ) -> Self::Item<'w> {
        *fetch
    }
}


/// Unique mutable borrow of an entity's component
#[derive(Debug)]
pub struct Mut<'a, T: ?Sized> {
    pub(crate) value: &'a mut T,
    pub(crate) row: Row,
    pub(crate) record: &'a ComponentRecord,
}
impl<'a, T: ?Sized> Deref for Mut<'a, T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl<'a, T: ?Sized> DerefMut for Mut<'a, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.record.changeds.len() > 0 {
            self.record.changed(self.row);
        }
        self.value
    }
}

macro_rules! impl_tuple_fetch {
    ($(($name: ident, $state: ident)),*) => {
        #[allow(non_snake_case)]
        #[allow(clippy::unused_unit)]
        // SAFETY: defers to soundness `$name: FetchComponents` impl
        impl<$($name: FetchComponents),*> FetchComponents for ($($name,)*) {
            type Fetch<'w> = ($($name::Fetch<'w>,)*);
            type Item<'w> = ($($name::Item<'w>,)*);
            type State = ($($name::State,)*);

            fn init_read_write(_world: &World, _rw: &mut ReadWrite) {
                ($($name::init_read_write(_world, _rw),)*);
            }
            fn archetype_filter(_archetype: &Archetype) -> bool {
                ($(
                    if $name::archetype_filter(_archetype){return true},
                )*);
                false
            }
            fn init_state(_world: &World, _archetype: &Archetype) -> Self::State {
                ($(
                    $name::init_state(_world, _archetype),
                )*)
            }

            #[inline(always)]
            #[allow(clippy::unused_unit)]
            fn init_fetch<'w>(_world: &'w World, _archetype: &'w Archetype, _state: &'w Self::State, _tick: Tick) -> Self::Fetch<'w> {
                let ($($state,)*) = _state;
                ($($name::init_fetch(_world, _archetype, $state, _tick),)*)
            }

            #[inline(always)]
            #[allow(clippy::unused_unit)]
            fn fetch<'w>(
                _fetch: &mut Self::Fetch<'w>,
                _state: &'w Self::State,
                _row: Row,
                _e: Entity,
            ) -> Self::Item<'w> {
                let ($($name,)*) = _fetch;
                let ($($state,)*) = _state;
                ($($name::fetch($name, $state, _row, _e),)*)
            }
        }

    };
}
all_tuples!(impl_tuple_fetch, 0, 15, F, S);
