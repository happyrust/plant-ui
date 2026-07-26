// 底标高
DEFINE FUNCTION fn::room_height($pe:record) {
    let $noun = $pe.refno.TYPE;
    return
    // EQUI可能有两层
    if $noun == 'EQUI' || $noun == 'STRU' || $noun == 'REST' {
        return array::distinct(
            (array::flatten([
        array::complement((select value (<-room_relate.in.id->inst_relate.aabb.d.mins[2])[0] from $pe<-pe_owner.in.id),[NONE]),
        array::complement((select value (<-room_relate.in.id->inst_relate.aabb.d.mins[2])[0] from $pe<-pe_owner.in<-pe_owner.in.id),[NONE])
        ])))
    } else if $noun == 'BRAN' {
        return array::complement((select value (<-room_relate.in.id->inst_relate.aabb.d.mins[2])[0] from $pe<-pe_owner.in.id), [none])
    } else  {
        return select value (<-room_relate.in.id->inst_relate.aabb.d.mins[2])[0] from $pe
    }
};

// 顶标高
DEFINE FUNCTION fn::room_top_height($pe:record) {
    let $noun = $pe.refno.TYPE;
    return
    // EQUI可能有两层
    if $noun == 'EQUI' || $noun == 'STRU' || $noun == 'REST' {
        return array::distinct(
            (array::flatten([
        array::complement((select value (<-room_relate.in.id->inst_relate.aabb.d.maxs[2])[0] from $pe<-pe_owner.in.id),[NONE]),
        array::complement((select value (<-room_relate.in.id->inst_relate.aabb.d.maxs[2])[0] from $pe<-pe_owner.in<-pe_owner.in.id),[NONE])
        ])))
    } else if $noun == 'BRAN' {
        return array::complement((select value (<-room_relate.in.id->inst_relate.aabb.d.maxs[2])[0] from $pe<-pe_owner.in.id), [none])
    } else  {
        return select value (<-room_relate.in.id->inst_relate.aabb.d.maxs[2])[0] from $pe
    }
};


// return fn::room_height(pe:24384_24804)[0];