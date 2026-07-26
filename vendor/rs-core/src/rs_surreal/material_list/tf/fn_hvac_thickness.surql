// 风管壁厚
remove function fn::hvac_thickness;
DEFINE FUNCTION fn::hvac_thickness($pe:record) {
    let $first = math::fixed($pe.refno.DESP[32]?:0.0,2)?:0.0;
    let $second = string::split($pe.owner.refno.ISPE.name?:'','-')[1]?:'';
    let $third = if fn::bran_3_b_6($pe.id) && array::first(fn::pre_or_next_b_type($pe.id,'DAMP')) { '3' } else { '' };
    return string::concat($first,',',$second,',',$third);
};

// 风管面积
remove function fn::hvac_area;
DEFINE FUNCTION fn::hvac_area($pe:record) {
    let $first = math::fixed($pe.refno.DESP[80]?:0.0 / 1000000,2) ;
    let $second = if $pe.owner.refno.ISPE != NONE { math::fixed(($pe.refno.DESP[80]?:0.0) / 1000000 ,2) } else { 0.0 };
    let $third = if fn::bran_3_b_6($pe) && array::first(fn::pre_or_next_b_type($pe,'DAMP')) { <string>math::fixed($pe.refno.DESP[1] * refno.DESP[2] * 150 / 1000000 ,2) } else { '' };
    return string::concat($first,',',<string>$second,',',$third);
};

/// 风管重量
remove function fn::hvac_weight;
DEFINE FUNCTION fn::hvac_weight($pe:record) {
    let $first = <string>math::fixed(refno.DESP[84]?:0.0,2);
    let $third = if fn::bran_3_b_6($pe) && array::first(fn::pre_or_next_b_type($pe,'DAMP')) { <string>math::fixed((refno.DESP[1]?:0.0 * refno.DESP[2]?:0.0 * 150 * refno.DESP[32]?:0.0 * 7.85) / 1000000,2) } else { '' } ;
    return string::concat($first,',',' ',',',$third);
};

// 风管壁厚
remove function fn::hvac_thickness_tape;
DEFINE FUNCTION fn::hvac_thickness_tape($pe:record) {
    let $first = math::fixed($pe.refno.DESP[32]?:0.0,2)?:0.0;
    let $second = string::split($pe.owner.refno.ISPE.name?:'','-')[1]?:'';
    return string::concat($first,',',$second);
};

// 风管面积
remove function fn::hvac_area_tape;
DEFINE FUNCTION fn::hvac_area_tape($pe:record) {
    let $first = math::fixed($pe.refno.DESP[80]?:0.0 / 1000000,2) ;
    let $second = if $pe.owner.refno.ISPE != NONE { math::fixed(($pe.refno.DESP[80]?:0.0) / 1000000 ,2) } else { 0.0 };
    return string::concat($first,',',<string>$second);
};

/// 风管重量
remove function fn::hvac_weight_tape;
DEFINE FUNCTION fn::hvac_weight_tape($pe:record) {
    let $first = <string>math::fixed(refno.DESP[84]?:0.0,2);
    return $first;
};

// 风管壁厚
remove function fn::hvac_thickness_bend;
DEFINE FUNCTION fn::hvac_thickness_bend($pe:record) {
    let $uda_value = string::split(fn::get_uda_value($pe.id, '/GuideVane')?:'',',')?:[];
    let $first = math::fixed($pe.refno.DESP[32]?:0.0,2)?:0.0;
    let $second = string::split($pe.owner.refno.ISPE.name?:'','-')[1]?:'';
    let $third =  $uda_value[0]?:'';
    return string::concat($first,',',$second,',',$third);
};

// 风管面积
remove function fn::hvac_area_bend;
DEFINE FUNCTION fn::hvac_area_bend($pe:record) {
    let $uda_value = string::split(fn::get_uda_value($pe.id, '/GuideVane')?:'',',')?:[];
    let $first = math::fixed($pe.refno.DESP[80]?:0.0 / 1000000,2) ;
    let $second = if $pe.owner.refno.ISPE != NONE { math::fixed(($pe.refno.DESP[80]?:0.0) / 1000000 ,2) } else { 0.0 };
    let $third = $uda_value[1]?:'';
    return string::concat($first,',',<string>$second,',',$third);
};

/// 风管重量
remove function fn::hvac_weight_bend;
DEFINE FUNCTION fn::hvac_weight_bend($pe:record) {
    let $uda_value = string::split(fn::get_uda_value($pe.id, '/GuideVane')?:'',',')?:[];
    let $first = <string>math::fixed(refno.DESP[84]?:0.0,2);
    let $third = $uda_value[2]?:'';
    return string::concat($first,',',$third);
};