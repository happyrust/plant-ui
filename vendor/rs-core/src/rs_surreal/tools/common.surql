
DEFINE FUNCTION fn::refno($pe: record){
    return string::replace(record::id($pe), '_', '/');
};

// 将已知的desp / para 数字转为 word
DEFINE FUNCTION fn::into_str($desp: float) {
    return if $desp == 788296 {
        'DIAM'
    } else if $desp == 531717 {
        'FJ'
    } else if $desp == 531501 {
        'FB'
    } else if $desp = 560742 {
        'FEMA'
    };
};

DEFINE FUNCTION fn::type($pe:record) {
    return $pe.noun;
};

// 判断上一个或下一个节点是否为 $type类型
DEFINE FUNCTION fn::prev_or_next_is_type($pe:record,$type:string){
    return fn::pre_is_type($pe,$type) || fn::next_is_type($pe,$type);
};

DEFINE FUNCTION fn::pre_is_type($pe:record, $type:string) {
    return fn::prev_type($pe)==$type;
};

//判断cref的类型
DEFINE FUNCTION fn::cref_is_type($pe:record, $type:string) {
    return $pe.refno.CREF.TYPE == $type;
};




// 获取room
DEFINE FUNCTION fn::get_room_number($pe:record) {
    return ($pe<-room_relate.room_num)[0];
}