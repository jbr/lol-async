initSidebarItems({"enum":[["Insertion","An `insert_with` operation result."]],"struct":[["IntoIter","An iterator over owned elements of a [`Set`]."],["Iter","An iterator over elements of a [`Set`]. The `Item` of this iterator is a [`ReadGuard`]."],["RandomState","`RandomState` is the default state for [`HashMap`] types."],["ReadGuard","A read-operation guard. This ensures no element allocation is mutated or freed while potential reads are performed."],["Removed","A removed element. It can be reinserted at the same [`Set`] it was removed. It can also be inserted on another [`Set`], but only if either the [`Set`] is dropped or there are no sensitive reads running on that [`Set`]."],["Set","A lock-free set. This is currently implemented on top of `Map`. To check more details about it, please see `Map` docs."],["SharedIncin","The shared incinerator used by [`Set`]. You may want to use this type in order to reduce memory consumption of the minimal space required by the incinerator. However, garbage items may be hold for longer time than they would if no shared incinerator were used."]]});