use core::fmt::*;
use core::result::Result;
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::mem::{transmute, ManuallyDrop};
use std::ops::DerefMut;

use crate::archetype::*;
use crate::record::RecordIndex;
use crate::fetch::FetchComponents;
use crate::filter::FilterComponents;
use crate::listener::Listener;
use crate::raw::{ArchetypeData, ArchetypePtr};
use crate::system::{ReadWrite, SystemMeta};
use crate::system_parms::SystemParam;
use crate::world::*;
use pi_arr::{Iter, RawIter};
use pi_null::*;
use pi_share::Share;
use smallvec::SmallVec;

#[derive(Debug)]
pub enum QueryError {
    /// The [`Query`] does not have read access to the requested component.
    ///
    /// This error occurs when the requested component is not included in the original query.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::{prelude::*, system::QueryComponentError};
    /// #
    /// # #[derive(Component)]
    /// # struct OtherComponent;
    /// #
    /// # #[derive(Component, PartialEq, Debug)]
    /// # struct RequestedComponent;
    /// #
    /// # #[derive(Resource)]
    /// # struct SpecificEntity {
    /// #     entity: Entity,
    /// # }
    /// #
    /// fn get_missing_read_access_error(query: Query<&OtherComponent>, res: Res<SpecificEntity>) {
    ///     assert_eq!(
    ///         query.get_component::<RequestedComponent>(res.entity),
    ///         Err(QueryComponentError::MissingReadAccess),
    ///     );
    ///     println!("query doesn't have read access to RequestedComponent because it does not appear in Query<&OtherComponent>");
    /// }
    /// # bevy_ecs::system::assert_is_system(get_missing_read_access_error);
    /// ```
    MissingReadAccess,
    /// The [`Query`] does not have write access to the requested component.
    ///
    /// This error occurs when the requested component is not included in the original query, or the mutability of the requested component is mismatched with the original query.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy_ecs::{prelude::*, system::QueryComponentError};
    /// #
    /// # #[derive(Component, PartialEq, Debug)]
    /// # struct RequestedComponent;
    /// #
    /// # #[derive(Resource)]
    /// # struct SpecificEntity {
    /// #     entity: Entity,
    /// # }
    /// #
    /// fn get_missing_write_access_error(mut query: Query<&RequestedComponent>, res: Res<SpecificEntity>) {
    ///     assert_eq!(
    ///         query.get_component::<RequestedComponent>(res.entity),
    ///         Err(QueryComponentError::MissingWriteAccess),
    ///     );
    ///     println!("query doesn't have write access to RequestedComponent because it doesn't have &mut in Query<&RequestedComponent>");
    /// }
    /// # bevy_ecs::system::assert_is_system(get_missing_write_access_error);
    /// ```
    MissingWriteAccess,
    /// The given [`Entity`] does not have the requested component.
    MissingComponent,
    /// The requested [`Entity`] does not exist.
    NoSuchEntity,
    NoSuchArchetype,
}

pub struct Query<'world, Q: FetchComponents + 'static, F: FilterComponents + 'static = ()> {
    world: &'world World,
    state: &'world mut QueryState<Q, F>,
    tick: Tick,
}

impl<'world, Q: FetchComponents, F: FilterComponents> Query<'world, Q, F> {
    pub fn new(world: &'world World, state: &'world mut QueryState<Q, F>, tick: Tick) -> Self {
        Query { world, state, tick }
    }
    pub fn get(
        &'world mut self,
        e: Entity,
    ) -> Result<<Q as FetchComponents>::Item<'world>, QueryError> {
        self.state.get(self.world, e, self.tick)
    }
    #[inline]
    pub fn tick(&self) -> Tick {
        self.tick
    }
    #[inline]
    pub fn entity_tick(&self, e: Entity) -> Tick {
        QueryState::<Q, F>::entity_tick(self.world, e)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }
    #[inline]
    pub fn delete(&mut self, e: Entity) -> Result<bool, QueryError> {
        self.state.delete(self.world, e)
    }
    pub fn iter(&'world self) -> QueryIter<'world, Q, F> {
        self.state.iter(self.world, self.tick)
    }
    // pub fn iter_mut(&'world mut self) -> QueryIterMut<'world, Q, F> {
    // } todo!()
}

// SAFETY: Relevant query ComponentId and ArchetypeComponentId access is applied to SystemMeta. If
// this Query conflicts with any prior access, a panic will occur.
impl<Q: FetchComponents + 'static, F: FilterComponents + Send + Sync + 'static> SystemParam
    for Query<'_, Q, F>
{
    type State = QueryState<Q, F>;
    type Item<'w> = Query<'w, Q, F>;

    fn init_state(world: &World, system_meta: &mut SystemMeta) -> Self::State {
        let mut rw = ReadWrite::default();
        Q::init_read_write(world, &mut rw);
        F::init_read_write(world, &mut rw);
        let c = rw.listeners.len();
        if c > 0 {
            // 遍历已有的原型， 添加record
            let notify = Notify(rw.listeners.clone(), PhantomData, PhantomData::<Q>, PhantomData::<F>);
            for (_, r) in world.archetype_arr.iter() {
                notify.listen(ArchetypeInit(r.as_ref().unwrap()))
            }
            // 监听原型创建， 添加record
            world.listener_mgr.register_event(Share::new(notify));
        }
        let i = system_meta.add_rw(rw);
        QueryState::new(i, c)
    }
    fn before(
        state: &mut Self::State,
        system_meta: &mut SystemMeta,
        world: &World,
        _change_tick: Tick,
    ) {
        state.align(world, system_meta);
    }

    #[inline]
    fn get_param<'world>(
        state: &'world mut Self::State,
        _system_meta: &'world SystemMeta,
        world: &'world World,
        change_tick: Tick,
    ) -> Self::Item<'world> {
        // SAFETY: We have registered all of the query's world accesses,
        // so the caller ensures that `world` has permission to access any
        // world data that the query needs.
        Query::new(world, state, change_tick)
    }
    fn after(
        state: &mut Self::State,
        _system_meta: &mut SystemMeta,
        _world: &World,
        _change_tick: Tick,
    ) {
        Self::State::clear(&state.vec, &mut state.removes);
    }
}

/// 监听原型创建， 添加record
struct Notify<'a, Q: FetchComponents, F: FilterComponents>(
    SmallVec<[(TypeId, bool); 1]>,
    PhantomData<&'a ()>,
    PhantomData<Q>,
    PhantomData<F>,
);
impl<'a, Q: FetchComponents + 'static, F: FilterComponents + 'static> Listener for Notify<'a, Q, F> {
    type Event = ArchetypeInit<'a>;
    fn listen(&self, ar: Self::Event) {
        if F::archetype_filter(ar.0) || Q::archetype_filter(ar.0) {
            return;
        }
        unsafe { ar.0.add_records(TypeId::of::<QueryState<Q, F>>(), &self.0) };
    }
}

#[derive(Debug)]
pub struct QueryState<Q: FetchComponents + 'static, F: FilterComponents + 'static> {
    rw_index: usize,
    listener_count: usize,
    vec: Vec<(ShareArchetype, SmallVec<[RecordIndex; 1]>)>, // 每原型及对应的记录监听
    state_vec: Vec<Q::State>,                              // 每原型对于的查询状态
    archetype_len: usize, // 记录的最新的原型，如果world上有更新的，则检查是否和自己相关
    map: HashMap<WorldArchetypeIndex, usize>, // 记录world上的原型索引对于本地的原型索引
    last: (WorldArchetypeIndex, usize), // todo!() 改到Query上来支持并发
    removes: Vec<(u32, ArchetypeKey)>, // 本次删除的本地原型位置及条目 // todo!() 改成AppendVec来支持并发 // 或者单加一个Delete来删除entity
    // key_check: HashSet<ArchetypeKey>, // todo!() 改到Iter上来支持并发
    _k: PhantomData<F>,
}
impl<Q: FetchComponents, F: FilterComponents> QueryState<Q, F> {
    pub fn new(rw_index: usize, listener_count: usize) -> Self {
        Self {
            rw_index,
            listener_count,
            vec: Vec::new(),
            state_vec: Vec::new(),
            archetype_len: 0,
            map: Default::default(),
            last: (WorldArchetypeIndex::null(), 0),
            removes: Vec::new(),
            _k: PhantomData,
        }
    }
    // 对齐world上新增的原型
    pub fn align(&mut self, world: &World, system_meta: &mut SystemMeta) {
        let len = world.archetype_arr.len();
        if len == self.archetype_len {
            return;
        }
        // 检查新增的原型
        for i in self.archetype_len..len {
            let ar = world.archetype_arr[i].as_ref().unwrap();
            self.add_archetype(world, ar, system_meta);
        }
        self.archetype_len = len;
    }
    // 新增的原型
    pub fn add_archetype(&mut self, world: &World, ar: &ShareArchetype, system_meta: &mut SystemMeta) {
        // 用With Without过滤原型, 在用查询提取的类型检查原型
        if F::archetype_filter(ar) || Q::archetype_filter(ar) {
            return;
        }
        let rw = &system_meta.get_rw(self.rw_index);
        let mut vec = SmallVec::new();
        if rw.listeners.len() > 0 {
            ar.find_records(TypeId::of::<Self>(), &rw.listeners, &mut vec);
            if vec.len() == 0 {
                // 表示该原型没有监听的组件，本查询可以不关心该原型
                return;
            }
        }
        self.vec.push((ar.clone(), vec));
        self.state_vec.push(Q::init_state(world, ar));
        system_meta.rw_archetype(self.rw_index, ar);
    }
    pub fn get<'w>(
        &'w mut self,
        world: &'w World,
        entity: Entity,
        tick: Tick,
    ) -> Result<Q::Item<'w>, QueryError> {
        let (k, v) = Self::check(&mut self.last, &self.map, world, entity)?;
        let ar = unsafe { &self.vec.get_unchecked(self.last.1).0 };
        let s = unsafe { self.state_vec.get_unchecked(self.last.1) };
        let mut fetch = Q::init_fetch(world, tick);
        Ok(Q::fetch(&mut fetch, ar, s, k, v))
    }
    pub fn entity_tick<'w>(world: &'w World, e: Entity) -> Tick {
        match world.entitys.get(e) {
            Some(v) => v.value().get_tick(),
            None => Tick::null(),
        }
    }
    /// 标记删除
    pub fn delete<'w>(&mut self, world: &'w World, entity: Entity) -> Result<bool, QueryError> {
        let (k, _) = Self::check(&mut self.last, &self.map, world, entity)?;
        let ars = unsafe { self.vec.get_unchecked(self.last.1) };
        if !ars.0.remove(k) {
            return Ok(false);
        }
        world.entitys.remove(entity).unwrap();
        self.removes.push((self.last.1 as u32, k));
        Ok(true)
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        if self.vec.is_empty() {
            return true;
        }
        self.len() == 0
    }
    pub fn len(&self) -> usize {
        let mut len = 0;
        for ar in &self.vec {
            len += ar.0.len();
        }
        len
    }
    pub fn iter<'w>(&'w self, world: &'w World, tick: Tick) -> QueryIter<'w, Q, F> {
        QueryIter::new(world, self, tick)
    }
    pub fn check<'w>(
        last: &mut (WorldArchetypeIndex, usize),
        map: &HashMap<WorldArchetypeIndex, usize>,
        world: &'w World,
        entity: Entity,
    ) -> Result<(ArchetypeKey, ArchetypeData), QueryError> {
        let value = match world.entitys.get(entity) {
            Some(v) => v,
            None => return Err(QueryError::NoSuchEntity),
        };
        let archetype_index = value.get_archetype().get_index();
        if last.0 != archetype_index {
            last.1 = match map.get(&archetype_index) {
                Some(v) => *v,
                None => return Err(QueryError::NoSuchArchetype),
            };
        }
        Ok((value.key(), value.value()))
    }
    fn clear(
        vec: &Vec<(ShareArchetype, SmallVec<[RecordIndex; 1]>)>,
        removes: &mut Vec<(u32, ArchetypeKey)>,
    ) {
        // 处理标记移除的条目
        while let Some((ar_index, key)) = removes.pop() {
            vec[ar_index as usize].0.drop_key(key);
        }
    }
}

/// 不同情况下的迭代器
union It<'w> {
    // 监听单个组件变化，只需对该组件的记录进行迭代
    record: ManuallyDrop<(&'w ShareArchetype, Iter<'w, ArchetypeKey>)>,
    // 监听多个组件变化，可能entity相同，需要进行去重
    records: ManuallyDrop<(
        &'w ShareArchetype,
        Iter<'w, ArchetypeKey>,
        HashSet<Entity>,
        &'w SmallVec<[RecordIndex; 1]>,
        usize,
    )>,
    // 没有监听变化，迭代该原型下所有的entity
    normal: ManuallyDrop<(&'w ShareArchetype, RawIter<'w>)>,
    // 停止
    none: (),
}
pub struct QueryIter<'w, Q: FetchComponents + 'static, F: FilterComponents + 'static> {
    state: &'w QueryState<Q, F>,
    fetch: Q::Fetch<'w>,
    tick: Tick,
    // 迭代器
    it: It<'w>,
    // 原型的位置， 如果为null，表示没有可迭代的原型
    ar_index: usize,
}
impl<'w, Q: FetchComponents, F: FilterComponents> QueryIter<'w, Q, F> {
    /// # Safety
    /// - `world` must have permission to access any of the components registered in `query_state`.
    /// - `world` must be the same one used to initialize `query_state`.
    pub(crate) fn new(world: &'w World, state: &'w QueryState<Q, F>, tick: Tick) -> Self {
        let len = state.vec.len();
        let (it, ar_index) = if len == 0 {
            // 该查询没有关联的原型
            (It { none: () }, usize::null())
        } else if state.listener_count == 0 {
            // 该查询没有监听组件变化
            // 倒序迭代所记录的原型
            let ar_index = len - 1;
            let ar = unsafe { &state.vec.get_unchecked(ar_index).0 };
            // println!("iter_normal!, start ar:{:?}", ar.get_id());
            (
                It {
                    normal: ManuallyDrop::new((ar, ar.iter())),
                },
                ar_index,
            )
        } else if state.listener_count == 1 {
            // 该查询没有只有1个组件变化监听器
            // 倒序迭代所记录的原型
            let ar_index = len - 1;
            let (ar, d) = unsafe { state.vec.get_unchecked(ar_index) };
            // 只有一个监听
            let d_index = unsafe { *d.get_unchecked(0) };
            (
                It {
                    record: ManuallyDrop::new((ar, d_index.get_iter(ar.get_records()))),
                },
                ar_index,
            )
        } else {
            // 该查询有多个组件变化监听器
            // 倒序迭代所记录的原型
            let ar_index = len - 1;
            let (ar, d) = unsafe { state.vec.get_unchecked(ar_index) };
            let d_index = unsafe { *d.get_unchecked(0) };
            (
                It {
                    records: ManuallyDrop::new((
                        ar,
                        d_index.get_iter(ar.get_records()),
                        HashSet::new(),
                        d,
                        1,
                    )),
                },
                ar_index,
            )
        };
        let fetch = Q::init_fetch(world, tick);
        QueryIter {
            state,
            fetch,
            tick,
            it,
            ar_index,
        }
    }
    fn iter_normal(
        vec: &'w Vec<(ShareArchetype, SmallVec<[RecordIndex; 1]>)>,
        it: &mut It<'w>,
        ar_index: &mut usize,
        tick: Tick,
    ) -> (&'w Archetype, ArchetypeKey, ArchetypeData) {
        let normal = unsafe { it.normal.deref_mut() };
        loop {
            match normal.1.next() {
                Some(r) => {
                    let data: ArchetypeData = unsafe { transmute(r.1) };
                    let t = data.get_tick();
                    // println!("iter_normal!, next r:{:?} t:{:?}", r.0, t);
                    // 要求条目不为空，同时不是本system修改的
                    if t > 0 && t < tick {
                        return (normal.0, r.0, data);
                    }
                }
                None => {
                    // 当前的原型已经迭代完毕
                    if *ar_index == 0 {
                        // 所有原型都迭代过了
                        *ar_index = usize::null();
                        return (normal.0, ArchetypeKey::null(), ArchetypeData::null());
                    }
                    // 下一个原型
                    *ar_index -= 1;
                    let ar = unsafe { &vec.get_unchecked(*ar_index).0 };
                    // println!("iter_normal!, replace ar:{:?}", ar.get_id());

                    *normal = (ar, ar.iter())
                }
            }
        }
    }
    fn iter_record(
        vec: &'w Vec<(ShareArchetype, SmallVec<[RecordIndex; 1]>)>,
        it: &mut It<'w>,
        ar_index: &mut usize,
        tick: Tick,
    ) -> (&'w Archetype, ArchetypeKey, ArchetypeData) {
        let ar_it = unsafe { it.record.deref_mut() };
        loop {
            match ar_it.1.next() {
                Some((_, k)) => {
                    let data: ArchetypeData = ar_it.0.get(*k);
                    if data.is_null() {
                        continue;
                    }
                    let t = data.get_tick();
                    // 要求条目不为空，同时不是本system修改的
                    if t > 0 && t < tick {
                        return (ar_it.0, *k, data);
                    }
                }
                None => {
                    // 当前的原型已经迭代完毕
                    if *ar_index == 0 {
                        // 所有原型都迭代过了
                        *ar_index = usize::null();
                        return (ar_it.0, ArchetypeKey::null(), ArchetypeData::null());
                    }
                    // 下一个原型
                    *ar_index -= 1;
                    let (ar, d) = unsafe { vec.get_unchecked(*ar_index) };
                    // 只监听一个组件的记录
                    let d_index = unsafe { d.get_unchecked(0) };
                    *ar_it = (ar, d_index.get_iter(ar.get_records()))
                }
            }
        }
    }

    fn iter_records(
        vec: &'w Vec<(ShareArchetype, SmallVec<[RecordIndex; 1]>)>,
        it: &mut It<'w>,
        ar_index: &mut usize,
        tick: Tick,
    ) -> (&'w Archetype, ArchetypeKey, ArchetypeData) {
        let ar_it = unsafe { it.records.deref_mut() };
        loop {
            match ar_it.1.next() {
                Some((_, k)) => {
                    let data: ArchetypeData = ar_it.0.get(*k);
                    if data.is_null() {
                        continue;
                    }
                    let t = data.get_tick();
                    // 如果条目为空，或者是本system修改的
                    if t == 0 && t == tick {
                        continue;
                    }
                    let entity = *data.entity();
                    // 如何和前面的entity重复，则跳过
                    if ar_it.2.contains(&entity) {
                        continue;
                    }
                    ar_it.2.insert(entity);
                    return (ar_it.0, *k, data);
                }
                None => {
                    // 检查当前原型的下一个被记录组件
                    if ar_it.3.len() > ar_it.4 {
                        let d_index = unsafe { *ar_it.3.get_unchecked(ar_it.4) };
                        ar_it.1 = d_index.get_iter(ar_it.0.get_records());
                        ar_it.4 += 1;
                        continue;
                    }
                    if *ar_index > 0 {
                        // 下一个原型
                        *ar_index -= 1;
                        let (ar, d) = unsafe { vec.get_unchecked(*ar_index) };
                        // 监听第一个被记录组件
                        let d_index = unsafe { *d.get_unchecked(0) };
                        ar_it.0 = ar;
                        ar_it.1 = d_index.get_iter(ar.get_records());
                        ar_it.3 = d;
                        ar_it.4 = 1;
                        continue;
                    }
                    // 所有原型都迭代过了
                    *ar_index = usize::null();
                    return (ar_it.0, ArchetypeKey::null(), ArchetypeData::null());
                }
            }
        }
    }
}

impl<'w, Q: FetchComponents, F: FilterComponents> Iterator for QueryIter<'w, Q, F> {
    type Item = Q::Item<'w>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.ar_index.is_null() {
            return None;
        }
        let (ar, key, ptr) = if self.state.listener_count == 0 {
            QueryIter::<Q, F>::iter_normal(
                &self.state.vec,
                &mut self.it,
                &mut self.ar_index,
                self.tick,
            )
        } else if self.state.listener_count == 1 {
            QueryIter::<Q, F>::iter_record(
                &self.state.vec,
                &mut self.it,
                &mut self.ar_index,
                self.tick,
            )
        } else {
            QueryIter::<Q, F>::iter_records(
                &self.state.vec,
                &mut self.it,
                &mut self.ar_index,
                self.tick,
            )
        };
        if key.is_null() {
            return None;
        }
        let item = Q::fetch(
            &mut self.fetch,
            ar,
            unsafe { &self.state.state_vec.get_unchecked(self.ar_index) },
            key,
            ptr,
        );
        return Some(item);
    }
}
