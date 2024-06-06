/// 在插入时， table上上分配了row，row的e为初始化状态，先写组件及record，然后再写world上entitys的ar_row，最后改table上的e为正确值的Entity。
/// 删除时， 是先改table上的e为删除状态，然后删除world上的entitys的e，最后销毁组件。
/// Alter移动时， 新table上分配了新row，先写移动相同的组件和新增组件及record，再改world上的entitys的ar_row，然后改旧table上row的e为删除状态，接着销毁旧table上的组件。最后改新table上row的e为正确值的Entity。
///
/// Alter所操作的源table， 在执行图中，会被严格保证不会同时有其他system进行操作。
use core::fmt::*;
use std::mem::replace;

use fixedbitset::FixedBitSet;
use pi_append_vec::AppendVec;
use pi_null::Null;
use pi_share::Share;

use crate::archetype::ArchetypeIndex;
use crate::archetype::Row;
use crate::column::Column;
use crate::world::{ComponentIndex, Entity, Tick, World};

pub struct Table {
    entities: AppendVec<Entity>, // 记录entity
    pub(crate) index: ArchetypeIndex,
    sorted_columns: Vec<Share<Column>>, // 每个组件
    bit_set: FixedBitSet,               // 记录组件是否在table中
    pub(crate) removes: AppendVec<Row>, // 整理前被移除的实例
}
impl Table {
    pub fn new(sorted_columns: Vec<Share<Column>>) -> Self {
        let len = sorted_columns.len();
        let max = if len > 0 {
            unsafe { sorted_columns.get_unchecked(len - 1).info().index.index() + 1 }
        } else {
            0
        };
        // let mut column_map = Vec::with_capacity(max);
        // column_map.resize(max, ColumnIndex::null());
        // let mut sorted_columns = Vec::with_capacity(len);
        let mut bit_set = FixedBitSet::with_capacity(max);
        for c in sorted_columns.iter() {
            unsafe { bit_set.set_unchecked(c.info().index.index(), true) };
        }
        Self {
            entities: AppendVec::default(),
            index: ArchetypeIndex::null(),
            sorted_columns,
            bit_set,
            // column_map: column_map,
            // remove_columns: Default::default(),
            // lock: SpinLock::new(()),
            // destroys: SyncUnsafeCell::new(Dirty::default()),
            removes: AppendVec::default(),
        }
    }
    /// Returns the number of elements in the archetype.
    #[inline(always)]
    pub fn len(&self) -> Row {
        Row(self.entities.len() as u32)
    }
    #[inline(always)]
    pub fn get(&self, row: Row) -> Entity {
        // todo 改成load_unchecked
        *self.entities.load(row.index()).unwrap()
    }
    #[inline(always)]
    pub fn set(&self, row: Row, e: Entity) {
        // todo 改成load_unchecked
        let a = self.entities.load(row.index()).unwrap();
        // println!("set1：{:p} {:p} {:?}", &self.entities, a, (&a, row, e, self.entities.vec_capacity(), self.entities.len()));
        *a = e;
    }
    #[inline(always)]
    pub fn get_columns(&self) -> &Vec<Share<Column>> {
        &self.sorted_columns
    }
    // 初始化原型对应列的blob
    pub fn init_blobs(&self) {
        for c in self.sorted_columns.iter() {
            c.init_blob(self.index);
        }
    }

    // 判断指定组件索引的组件是否在table中
    #[inline(always)]
    pub fn contains(&self, index: ComponentIndex) -> bool {
        self.bit_set.contains(index.index())
    }
    // pub fn get_column_index_by_tid(&self, world: &World, tid: &TypeId) -> ColumnIndex {
    //     self.get_column_index(world.get_component_index(tid))
    // }
    // pub fn get_column_by_tid(&self, world: &World, tid: &TypeId) -> Option<(&Column, ColumnIndex)> {
    //     self.get_column(world.get_component_index(tid))
    // }
    // pub fn get_column_index(&self, index: ComponentIndex) -> ColumnIndex {
    //     self.column_map.get(index.index()).map_or(ColumnIndex::null(), |r| *r)
    // }
    // pub fn get_column(&self, index: ComponentIndex) -> Option<(&Column, ColumnIndex)> {
    //     // println!("get_column：{:?}", index);
    //     if let Some(t) = self.column_map.get(index.index()) {
    //         // println!("get_column1：{:?}", t);
    //         if t.is_null() {
    //             return None;
    //         }
    //         let c = self.get_column_unchecked(*t);
    //         return Some((c, *t));
    //     }
    //     None
    // }
    // pub(crate) unsafe fn get_column_mut(
    //     &self,
    //     index: ComponentIndex,
    // ) -> Option<(&mut Column, ColumnIndex)> {
    //     if let Some(t) = self.column_map.get(index.index()) {
    //         if t.is_null() {
    //             return None;
    //         }
    //         let c = self.get_column_unchecked(*t);
    //         return unsafe { transmute(Some((c, *t))) };
    //     }
    //     None
    // }
    pub(crate) fn get_column_unchecked(&self, index: usize) -> &Share<Column> {
        unsafe { self.sorted_columns.get_unchecked(index) }
    }
    // /// 添加changed监听器，原型刚创建时调用
    // pub fn add_listener(&self, index: ComponentIndex, owner: Tick,
    //     tick: Share<ShareUsize>,) {
    //     // println!("add_changed_listener!! self: {:p}, index: {:?}", self, index);
    //     if let Some((c, _)) = unsafe { self.get_column_mut(index) } {
    //         c.dirty.insert_listener(owner, tick)
    //     }
    // }
    // /// 添加removed监听器，原型刚创建时调用
    // pub fn add_removed_listener(&self, index: ComponentIndex, owner: Tick) {
    //     // 获取索引
    //     let column_index = self.add_remove_column_index(index);
    //     let r = unsafe { self.remove_columns.load_unchecked(column_index.index()) };
    //     // 添加新的监听
    //     r.dirty.insert_listener(owner);
    // }
    /// 添加destroyed监听器，原型刚创建时调用
    // pub fn add_destroyed_listener(&self, owner: Tick) {
    //     unsafe { &mut *self.destroys.get() }.insert_listener(owner)
    // }
    // /// 查询在同步到原型时，寻找自己添加的changed监听器，并记录组件位置和监听器位置
    // pub(crate) fn find_listener(
    //     &self,
    //     index: ComponentIndex,
    //     owner: Tick,
    //     result: &mut Vec<(ColumnIndex, Share<ShareUsize>, Share<AppendVec<EntityRow>>)>,
    // ) {
    //     if let Some((c, column_index)) = self.get_column(index) {
    //         c.dirty.find_listener(column_index, owner, result);
    //     }
    // }
    // /// 查询在同步到原型时，寻找自己添加的removed监听器，并记录组件位置和监听器位置
    // pub(crate) fn find_removed_listener(
    //     &self,
    //     index: ComponentIndex,
    //     owner: Tick,
    //     vec: &mut Vec<DirtyIndex>,
    // ) {
    //     let column_index = self.add_remove_column_index(index);
    //     let r = self.get_remove_column(column_index);
    //     let listener_index = r.dirty.find_listener_index(owner);
    //     if !listener_index.is_null() {
    //         vec.push(DirtyIndex {
    //             listener_index,
    //             dtype: DirtyType::Removed(column_index.into()),
    //         });
    //     }
    // }
    /// 查询在同步到原型时，寻找自己添加的destroyed监听器，并记录监听器位置
    // pub(crate) fn find_destroyed_listener(
    //     &self,
    //     owner: Tick,
    //     vec: &mut Vec<DirtyIndex>,
    // ) {
    //     let list = unsafe { &*self.destroys.get() };
    //     let listener_index = list.find_listener_index(owner);
    //     if !listener_index.is_null() {
    //         vec.push(DirtyIndex {
    //             listener_index,
    //             dtype: DirtyType::Destroyed,
    //         });
    //     }
    // }

    // /// 获得对应的脏列表, 及是否不检查entity是否存在
    // pub(crate) fn get_dirty_iter<'a>(&'a self, dirty_index: &DirtyIndex, tick: Tick) -> DirtyIter<'a> {
    //      match dirty_index.dtype {
    //         DirtyType::Destroyed => {
    //             let r = unsafe { &*self.destroys.get() };
    //             DirtyIter::new(r.get_iter(dirty_index.listener_index, Tick::null()), None)},
    //         DirtyType::Changed(column_index) => {
    //             let r = self.get_column_unchecked(column_index);
    //             DirtyIter::new(r.dirty.get_iter(dirty_index.listener_index, tick), Some(&r.ticks))
    //         },
    //     }
    // }

    /// 扩容
    pub fn reserve(&mut self, additional: usize) {
        let len = self.entities.len();
        self.entities.settle(additional);
        let vec = vec![];
        self.settle_columns(len, additional, &vec);
    }
    /// 整理每个列
    pub(crate) fn settle_columns(&mut self, len: usize, additional: usize, vec: &Vec<(Row, Row)>) {
        // println!("Table settle_columns, {:?}", (self.index, len));
        for c in self.sorted_columns.iter_mut() {
            let c = unsafe { Share::get_mut_unchecked(c) };
            c.settle_by_index(self.index, len, additional, vec);
        }
    }

    #[inline(always)]
    pub fn alloc(&self) -> (&mut Entity, usize) {
        self.entities.alloc()
    }
    /// 销毁，用于destroy
    pub(crate) fn destroy(&self, row: Row) -> Entity {
        // todo 改成load_unchecked
        let e = self.entities.load(row.index()).unwrap();
        if e.is_null() {
            return *e;
        }
        for c in self.sorted_columns.iter() {
            let c = c.blob_ref_unchecked(self.index);
            c.drop_row(row);
        }
        self.removes.insert(row);
        replace(e, Entity::null())
    }
    /// 标记移出，用于alter
    /// mark removes a key from the archetype, returning the value at the key if the
    /// key was not previously removed.
    pub(crate) fn mark_remove(&self, row: Row) -> Entity {
        // todo 改成load_unchecked
        let e = self.entities.load(row.index()).unwrap();
        if e.is_null() {
            return *e;
        }
        self.removes.insert(row);
        replace(e, Entity::null())
    }
    /// 初始化一个行，每个列都插入一个默认值
    pub(crate) fn init_row(&self, row: Row, e: Entity, tick: Tick) {
        for column in &self.sorted_columns {
            let c = column.blob_ref_unchecked(self.index);
            let dst_data: *mut u8 = unsafe { c.load(row) };
            column.info().default_fn.unwrap()(dst_data);
            c.added_tick(e, row, tick)
        }
    }

    // // 处理标记移除的条目，返回true，表示所有监听器都已经处理完毕，然后可以清理destroys
    // fn clear_destroy(&mut self) -> bool {
    //     let dirty = unsafe { &mut *self.destroys.get() };
    //     let len = match dirty.can_clear() {
    //         Some(len) => {
    //             if len == 0 {
    //                 return true;
    //             }
    //             len
    //         }
    //         _ => return false,
    //     };
    //     for e in dirty.vec.iter() {
    //         self.removes.insert(e.row);
    //     }
    //     self.drop_vec(&dirty.vec);
    //     dirty.clear(len);
    //     true
    // }
    /// 删除全部组件
    // pub(crate) fn drop_vec(&self, vec: &AppendVec<EntityRow>) {
    //     for t in self.sorted_columns.iter() {
    //         for e in vec.iter() {
    //             t.drop_row(e.row);
    //         }
    //     }
    // }
    /// 获得移除数组产生的动作， 返回新entitys的长度
    pub(crate) fn removes_action(
        removes: &AppendVec<Row>,
        remove_len: usize,
        entity_len: usize,
        action: &mut Vec<(Row, Row)>,
        set: &mut FixedBitSet,
    ) -> usize {
        action.clear();
        // 根据4种情况， 获得新长度new_entity_len，并且在action中放置了移动对
        if remove_len >= entity_len {
            // 全部移除
            return 0;
        }
        if remove_len == 1 {
            // 移除一个，用交换尾部的方式
            let remove_row = unsafe { removes.get_unchecked(0) };
            if remove_row.index() + 1 < entity_len {
                action.push((Row(entity_len as u32 - 1), *remove_row));
            }
            return entity_len - 1;
        }
        let r = remove_len as f64;
        if r * r.log2() < (entity_len - remove_len) as f64 {
            // 少量移除， 走removes排序，计算好移动对
            // 需要扫描removes一次，排序一次，再扫描action一次, 消耗为n*log2n+n
            // 先将removes的数据放入action，然后排序
            for row in removes.iter() {
                action.push((*row, *row));
            }
            action.sort_unstable();
            // 按从后移动到前的方式，计算移动对
            let mut start = 0;
            let mut end = action.len();
            let mut index = entity_len;
            while start < end {
                index -= 1;
                let remove_row = unsafe { action.get_unchecked(end - 1) };
                if remove_row.0.index() == index {
                    // 最大的要移动的行就是entitys的最后一个，则跳过
                    end -= 1;
                    continue;
                }
                // 移动到前面
                let r = unsafe { action.get_unchecked_mut(start) };
                r.0 = Row(index as u32);
                start += 1;
            }
            action.truncate(end);
            return index;
        }
        // 大量移除，走fixbitset的位标记方式，再次扫描，计算移动对
        // 需要扫描removes一次，entitys一次, 消耗为entity_len
        set.clear();
        set.grow(entity_len);
        for row in removes.iter() {
            set.set(row.index(), true);
        }
        let ones = set.ones();
        let mut end = entity_len;
        for row in ones {
            // 找到最后一个未被移除的
            loop {
                if row >= end {
                    return end;
                }
                end -= 1;
                if !set.contains(end) {
                    // 放入移动对
                    action.push((Row(end as u32), Row(row as u32)));
                    break;
                }
            }
        }
        end
    }
    /// 只有主调度完毕后，才能调用的整理方法
    /// 尝试清空所有列的脏列表，所有的脏都被成功的处理和清理后，才能进行row调整
    /// 调整Row，将空位的entity换到尾部，将entitys变紧凑，没有空位。
    /// 在整理前，Row都是递增的。
    pub(crate) fn settle(
        &mut self,
        world: &World,
        action: &mut Vec<(Row, Row)>,
        set: &mut FixedBitSet,
    ) -> bool {
        // if !self.clear_destroy() {
        //     // 如果清理destroys不成功，不调整row，返回
        //     return false;
        // }
        // let mut r = true;
        // // 先整理每个列，如果所有列的脏列表成功清空
        // for c in self.sorted_columns.iter_mut() {
        //     r &= c.dirty.settle();
        // }
        // if !r {
        //     // 有失败的脏，不调整row，返回
        //     return false;
        // }
        // // 整理全部的remove_columns，如果所有移除列的脏列表成功清空
        // for d in self.remove_columns.iter() {
        //     r &= d.dirty.collect();
        // }
        // if !r {
        //     // 有失败的脏，不调整row，返回
        //     return false;
        // }
        let remove_len = self.removes.len();
        if remove_len == 0 {
            return true;
        }
        let new_entity_len =
            Self::removes_action(&self.removes, remove_len, self.entities.len(), action, set);
        // 清理removes
        self.removes.clear(0);
        // 整理全部的列, 合并空位
        self.settle_columns(new_entity_len, 0, &action);
        // // 整理全部的列ticks
        // for c in self.sorted_columns.iter_mut() {
        //     if c.info().is_tick() {
        //         settle_ticks(&mut c.ticks, new_entity_len, &action);
        //     }
        // }
        // // 整理全部的RemovedColumn列ticks
        // for c in self.remove_columns.iter() {
        //     collect_ticks(&mut c.ticks, new_entity_len, &action);
        // }
        // 再移动entitys的空位
        for (src, dst) in action.iter() {
            let e =
                unsafe { replace(self.entities.get_unchecked_mut(src.index()), Entity::null()) };
            *unsafe { self.entities.get_unchecked_mut(dst.index()) } = e;
            // 修改world上entity的地址
            world.replace_row(e, *dst);
        }
        // 设置成正确的长度
        unsafe {
            self.entities.set_len(new_entity_len);
        };
        // 整理合并内存
        self.entities.settle(0);
        true
    }
}
impl Drop for Table {
    fn drop(&mut self) {
        println!("drop table {:?}", self.index);
        // 释放每个列中还存在的row
        let len = self.len().index();
        if len == 0 {
            return;
        }
        for c in self.sorted_columns.iter_mut() {
            if c.info().drop_fn.is_none() {
                continue;
            }
            let c = c.blob_ref(self.index).unwrap();
            // 释放每个列中还存在的row
            for (row, e) in self.entities.iter().enumerate() {
                if !e.is_null() {
                    c.drop_row_unchecked(Row(row as u32));
                }
            }
        }
    }
}

impl Debug for Table {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("Table")
            .field("entitys", &self.entities)
            .field("sorted_columns", &self.sorted_columns)
            .field("removes", &self.removes)
            .finish()
    }
}

// /// 整理合并空位
// pub(crate) fn settle_ticks(ticks: &mut Arr<Tick>, entity_len: usize, action: &Vec<(Row, Row)>) {
//     for (src, dst) in action.iter() {
//         if let Some(tick) = ticks.get(src.index()) {
//             *ticks.load(dst.index()).unwrap() = *tick;
//         }
//     }
//     ticks.settle(entity_len, 0, 1);
// }
