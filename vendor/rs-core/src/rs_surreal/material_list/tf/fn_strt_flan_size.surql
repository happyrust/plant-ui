// 法兰规格
remove function fn::strt_flan_size;
DEFINE FUNCTION fn::strt_flan_size($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_fls = $desp[67];
    let $fl_type = if $desp[57] == 531717 { 'FJ' } else if $desp[57] == 531501 { 'FB' } else { '' };
    return select size from tf_strt_flan_size where fl_type = $fl_type and fl_size = $fl_fls;
};

remove function fn::strt_flan_size_2;
DEFINE FUNCTION fn::strt_flan_size_2($pe:record) {
    let $desp = $pe.refno.DESP;
   let $fl_fls = $desp[67];
    let $fl_fls_2 = $desp[68];
    let $fl_type = if $desp[58] == 531717 { 'FJ' } else if $desp[58] == 531501 { 'FB' } else { '' };
    return if $fl_fls == $fl_fls_2 { return '' } else { return select size from tf_strt_flan_size where fl_type = $fl_type and fl_size = $fl_fls_2 };
};

remove function fn::strt_flan_size_thre_3;
DEFINE FUNCTION fn::strt_flan_size_thre_3($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_fls_2 = $desp[69];
    let $fl_type = if $desp[59] == 531717 { 'FJ' } else if $desp[58] == 531501 { 'FB' } else { '' };
    return select size from tf_strt_flan_size where fl_type = $fl_type and fl_size = $fl_fls_2;
};

remove function fn::strt_flan_size_trns_2;
DEFINE FUNCTION fn::strt_flan_size_trns_2($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_fls = $desp[67];
    let $fl_fls_2 = $desp[68];
    let $fl_type = if $desp[58] == 531717 { 'FJ' } else if $desp[58] == 531501 { 'FB' } else { '' };
    return select size from tf_strt_flan_size where fl_type = $fl_type and fl_size = $fl_fls_2;
};


// 法兰规格
remove function fn::flan_steel;
define FUNCTION fn::flan_steel($pe:record) {
    return string::concat(fn::strt_flan_size($pe.id)[0].size?:'',' , ',fn::strt_flan_size_2($pe.id)[0].size?:'')
};

// 法兰规格
remove function fn::flan_steel_trns;
define FUNCTION fn::flan_steel_trns($pe:record) {
    return string::concat(fn::strt_flan_size($pe.id)[0].size?:'',' , ',fn::strt_flan_size_trns_2($pe.id)[0].size?:'')
};

remove function fn::flan_steel_thre;
define FUNCTION fn::flan_steel_thre($pe:record) {
    return string::concat(fn::strt_flan_size($pe.id)[0].size?:'',' , ',fn::strt_flan_size_trns_2($pe.id)[0].size?:'',' , ',fn::strt_flan_size_thre_3($pe.id)[0].size?:'')
};