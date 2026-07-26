remove function fn::strt_flan_length_1;
DEFINE FUNCTION fn::strt_flan_length_1($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_type = if $desp[57] == 531717 { 'FJ' } else if $desp[57] == 531501 { 'FB' } else { '' };
    let $b_fl_type = if $fl_type == '' { false } else { true };
    let $fl_shape = fn::hvac_shape($pe);
    let $fl_fls = $desp[67];
    let $fl_fls_2 = $desp[68];

    let $width = $pe.refno.DESP[1]?:0;
    let $height = $pe.refno.DESP[2]?:0;
    let $fl_width = $pe.refno.DESP[20]?:0;

    return if $fl_shape == 'RECT' && $fl_fls == $fl_fls_2 && $b_fl_type {
       return math::fixed(( $width + $height + $fl_width * 2 ) * 4 /1000,2)
    } else if $fl_shape == 'CIRC' && $fl_fls == $fl_fls_2 && $b_fl_type {
       return math::fixed(( $width + $fl_width * 2 ) * 6.28 /1000,2)
    } else if $fl_shape == 'RECT' && $fl_fls != $fl_fls_2 && $b_fl_type {
       return math::fixed(( $width + $height + $fl_width * 2 ) * 2 /1000,2)
    } else if $fl_shape == 'CIRC' && $fl_fls != $fl_fls_2 && $b_fl_type {
       return math::fixed(( $width + $fl_width * 2 ) * 3.14 /1000,2)
    }
};


remove function fn::strt_flan_length_2;
DEFINE FUNCTION fn::strt_flan_length_2($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_type = if $desp[58] == 531717 { 'FJ' } else if $desp[58] == 531501 { 'FB' } else { '' };
    let $b_fl_type = if $fl_type == '' { false } else { true };
    let $fl_shape = fn::hvac_shape_2($pe);
    let $fl_fls = $desp[67];
    let $fl_fls_2 = $desp[68];

    let $width = $pe.refno.DESP[1]?:0;
    let $height = $pe.refno.DESP[2]?:0;
    let $fl_width_2 = $pe.refno.DESP[23]?:0;

    return if $fl_shape == 'RECT' && $fl_fls != $fl_fls_2 && $b_fl_type {
       return math::fixed(( $width + $height + $fl_width_2 * 2 ) * 2 /1000,2)
    } else if $fl_shape == 'CIRC' && $fl_fls != $fl_fls_2 && $b_fl_type {
       return math::fixed(( $width + $fl_width_2 * 2 ) * 3.14 /1000,2)
    }
};

// 法兰长度
remove function fn::strt_flan_length;
DEFINE FUNCTION fn::strt_flan_length($pe:record) {
    return string::concat(<string>(fn::strt_flan_length_1($pe)?:''),',',<string>(fn::strt_flan_length_2($pe)?:''))
};

// return fn::strt_flan_length(pe:24381_46952);

remove function fn::strt_flan_length_trns_1;
DEFINE FUNCTION fn::strt_flan_length_trns_1($pe:record) {
    let $desp = $pe.refno.DESP;
    let $width = $desp[1]?:0;
    let $height = $desp[2]?:0;
    let $fl_width = $desp[20]?:0;
    let $height_str = string::e3d_dehash($height);

    return if $height_str == 'DIAM' {
        return math::fixed(($width + $fl_width * 2 ) * 3.14 / 1000 ,2)
    } else {
        return math::fixed(($width + $height + $fl_width * 2) * 2 / 1000  ,2)
     }
};

remove function fn::strt_flan_length_trns_2;
DEFINE FUNCTION fn::strt_flan_length_trns_2($pe:record) {
    let $desp = $pe.refno.DESP;
    let $width_2 = $desp[3]?:0;
    let $height_2 = $desp[4]?:0;
    let $fl_width_2 = $desp[23]?:0;
    let $height_str_2 = string::e3d_dehash($height_2);

    return if $height_str_2 == 'DIAM' {
        return math::fixed(($width_2 + $fl_width_2 * 2 ) * 3.14 / 1000 ,2)
    } else {
        return math::fixed(($width_2 + $height_2 + $fl_width_2 * 2) * 2 / 1000  ,2)
     }
};

// 法兰长度
remove function fn::strt_flan_length_trns;
DEFINE FUNCTION fn::strt_flan_length_trns($pe:record) {
    return string::concat(<string>(fn::strt_flan_length_trns_1($pe)?:''),',',<string>(fn::strt_flan_length_trns_2($pe)?:''))
};

remove function fn::strt_flan_length_thre_1;
DEFINE FUNCTION fn::strt_flan_length_thre_1($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_shape = fn::hvac_shape($pe);
    let $width = $desp[1]?:0;
    let $height = $desp[2]?:0;
    let $fl_width = $desp[20]?:0;

    return if $fl_shape == 'CIRC' {
        return math::fixed(($width + $fl_width * 2 ) * 3.14 / 1000 ,2)
    } else if $fl_shape == 'RECT' {
        return math::fixed(($width + $height + $fl_width * 2) * 2 / 1000  ,2)
     }
};

remove function fn::strt_flan_length_thre_2;
DEFINE FUNCTION fn::strt_flan_length_thre_2($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_shape = fn::hvac_shape($pe);
    let $width_2 = $desp[3]?:0;
    let $fl_width_2 = $desp[23]?:0;

    return if $fl_shape == 'CIRC' {
        return math::fixed(($width_2 + $fl_width_2 * 2 ) * 3.14 / 1000 ,2)
    } else if $fl_shape == 'RECT' {
        return math::fixed(($width_2 + $fl_width_2 * 2) * 2 / 1000  ,2)
     }
};

remove function fn::strt_flan_length_thre_3;
DEFINE FUNCTION fn::strt_flan_length_thre_3($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_shape = fn::hvac_shape($pe);
    let $width_3 = $desp[40]?:0;
    let $height_3 = $desp[41]?:0;
    let $fl_width_3 = $desp[28]?:0;

    return if $fl_shape == 'CIRC' {
        return math::fixed(($width_3 + $fl_width_3 * 2 ) * 3.14 / 1000 ,2)
    } else if $fl_shape == 'RECT' {
        return math::fixed(($width_3 + $height_3 + $fl_width_3 * 2) * 2 / 1000  ,2)
     }
};

// 法兰长度
remove function fn::strt_flan_length_thre;
DEFINE FUNCTION fn::strt_flan_length_thre($pe:record) {
    return string::concat(<string>(fn::strt_flan_length_thre_1($pe)?:''),',',<string>(fn::strt_flan_length_thre_2($pe)?:''),',',<string>(fn::strt_flan_length_thre_3($pe)?:''))
};