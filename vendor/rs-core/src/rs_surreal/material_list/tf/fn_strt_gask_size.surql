
remove function fn::strt_gask_size_1;
DEFINE FUNCTION fn::strt_gask_size_1($pe:record) {
    let $uda_value = fn::get_uda_value($pe.id, '/GasketStatis');
    return if $uda_value != NONE {
        return string::split($uda_value,',')[0]?:''
    } else {
        ''
    }
};

remove function fn::strt_gask_size_2;
DEFINE FUNCTION fn::strt_gask_size_2($pe:record) {
    let $uda_value = fn::get_uda_value($pe.id, '/GasketStatis');
    return if $uda_value != NONE {
        return string::split($uda_value,',')[2]?:''
    } else {
        ''
    }
};

// 垫片类型
remove function fn::strt_gask_size;
DEFINE FUNCTION fn::strt_gask_size($pe:record) {
    return string::concat(fn::strt_gask_size_1($pe),',',fn::strt_gask_size_2($pe));
};

remove function fn::strt_gask_length_1;
DEFINE FUNCTION fn::strt_gask_length_1($pe:record) {
    let $uda_value = fn::get_uda_value($pe.id, '/GasketStatis');
    return if $uda_value != NONE {
        return string::split($uda_value,',')[0]?:''
    } else {
        ''
    }
};

remove function fn::strt_gask_length_2;
DEFINE FUNCTION fn::strt_gask_length_2($pe:record) {
    let $uda_value = fn::get_uda_value($pe.id, '/GasketStatis');
    return if $uda_value != NONE {
        return string::split($uda_value,',')[2]?:''
    } else {
        ''
    }
};

// 垫片长度
remove function fn::strt_gask_length;
DEFINE FUNCTION fn::strt_gask_length($pe:record) {
    return string::concat(fn::strt_gask_length_1($pe),',',fn::strt_gask_length_2($pe));
};