//TRNS
remove function fn::fggd_TRNS;
define function fn::fggd_TRNS($pes:array<record>) {
    return select
        fn::pe_to_refno(id) as id, // 参考号
        fn::fggd_bran_code(id)[0] as bran_seg_code, // 管段编号
        string::concat(string::replace(string::split(owner.name,'-')[4]?:'','L','B'),'P') as pressure_level, // 压力等级
        fn::hvac_width_trns(id) as w, // 宽
        fn::hvac_height_trns(id) as h, // 高
        fn::hvac_thickness_tape(id)?:'' as thickness, // 风管壁厚
        fn::hvac_area_tape(id)?:'' as area, // 风管面积
        fn::hvac_weight_tape(id)?:'' as weight,  // 风管总重
        fn::stif_sctn(id) as stif_sctn, // 风管加强 型钢
        fn::stif_len(id) as stif_len, // 风管加强 长度
        fn::stif_wei(id) as stif_wei, //// 风管加强 重量
        fn::flan_steel_trns(id)?:'' as flan_steel, // 法兰 型钢
        fn::strt_flan_length_trns(id)?:'' as flan_length,//法兰 长度
        fn::strt_flan_weight_trns(id)?:'' as flan_weight, //法兰 总重
        fn::strt_gask_size($this.id)?:'' as gask_size, //垫片类型
        fn::strt_gask_length($this.id)?:'' as shim_width, // 垫片 长度
        fn::strt_bolt_count($this.id)?:'' as bolt_count,  // 螺栓数量
        '' as other_material_count, // 其它材料数量
        '' as screw, // 螺杆
        '' as nut_count, // 螺母数量
        fn::room_code(id)[0]?:'' as room_num , // 房间号
        array::join([fn::hvac_material($this.id),fn::get_ispec_material($this.id)],',') as material // 材质
    from $pes;
};

