// 加强筋型材
remove function fn::stif_sctn;
define function fn::stif_sctn($pe:record) {
    let $uda_value = fn::get_uda_value($pe.id, '/Strengthen');
    return if $uda_value != NONE && $uda_value != 'unset' {
        array::at(string::split($uda_value,','),0)
    } else {
        ''
    }
};

// 加强筋长度
remove function fn::stif_len;
define function fn::stif_len($pe:record) {
    let $uda_value = fn::get_uda_value($pe.id, '/Strengthen');
    return if $uda_value != NONE && $uda_value != 'unset' {
        array::at(string::split($uda_value,','),2) / 1000
    } else {
        0.0
    }
};

// 加强筋重量
remove function fn::stif_wei;
define function fn::stif_wei($pe:record) {
    let $uda_value = fn::get_uda_value($pe.id, '/Strengthen');
    return if $uda_value != NONE && $uda_value != 'unset' {
        array::at(string::split($uda_value,','),3) / 1000
    } else {
        0.0
    }
};

remove function fn::stif_sctn_stif;
define function fn::stif_sctn_stif($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_width_1 = <string>math::fixed($desp[26]?:0,2);
    let $fl_width_2 = <string>math::fixed($desp[27]?:0,2);
    let $fl_width_3 = <string>math::fixed($desp[28]?:0,2);
    return string::concat('L',$fl_width_1,'X',$fl_width_2,'X',$fl_width_3)
};

remove function fn::stif_len_stif;
define function fn::stif_len_stif($pe:record) {
    let $desp = $pe.refno.DESP;
    let $fl_shape = fn::hvac_shape($pe);

    let $width = $pe.refno.DESP[1]?:0.0;
    let $height = $pe.refno.DESP[2]?:0.0;
    let $fl_width = $pe.refno.DESP[26]?:0.0;

    return if $fl_shape == 'RECT' {
       return math::fixed(( $width + $height + $fl_width * 2 ) * 2 /1000,2)
    } else {
       return math::fixed(( $width + $fl_width * 2 ) * 3.14 /1000,2)
    }
};

remove function fn::stif_wei_stif;
define function fn::stif_wei_stif($pe:record) {
    let $length = fn::stif_len_stif($pe.id);
    let $fl_width_1 = <string>math::fixed($desp[26]?:0,2);
    let $fl_width_2 = <string>math::fixed($desp[27]?:0,2);
    let $fl_width_3 = <string>math::fixed($desp[28]?:0,2);
    return $length * ($fl_width_1 + $fl_width_2 - $fl_width_3) * $fl_width_3 * 7.85 / 1000
}