remove function fn::fggd_STIF;
define function fn::fggd_STIF($pes:array<record>) {
    return select
        fn::pe_to_refno(id) as id, // 参考号
        fn::fggd_bran_code(id)[0] as bran_seg_code, // 管段编号
        string::concat(string::replace(string::split(owner.name,'-')[4]?:'','L','B'),'P') as pressure_level, // 压力等级
        fn::stif_sctn_stif(id) as stif_sctn, // 风管加强 型钢
        fn::stif_len_stif(id) as stif_len, // 风管加强 长度
        fn::stif_wei_stif(id) as stif_wei, //// 风管加强 重量
        '' as other_material_count, // 其它材料数量
        '' as screw, // 螺杆
        '' as nut_count, // 螺母数量
        array::join([fn::hvac_material($this.id),fn::get_ispec_material($this.id)],',') as material // 材质
    from $pes;
};

