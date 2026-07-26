
remove function fn::collect_mat_statistic;
DEFINE FUNCTION fn::collect_mat_statistic($pe: record){
    let $type = $pe.noun;
    let $o_type = $pe.owner.noun;
    let $oo_type = $pe.owner.owner.noun;
    return if $type == 'SITE' {
        return fn::collect_hvac_SITE($pe);
    } else if $type == 'ZONE' {
        return fn::collect_hvac_ZONE($pe);
    } else if $type == 'HVAC' {
        return fn::collect_hvac($pe);
    } else if $type == 'BRAN' && $o_type == 'HVAC' {
        return fn::collect_hvac_bran($pe);
    } else if $o_type == 'BRAN' && $oo_type == 'HVAC' {
        return fn::collect_hvac_ele($pe);
    };
}

