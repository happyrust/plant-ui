
DEFINE FUNCTION fn::create_mdb_sites($mdb: string, $db_type){

    let $dbnos = select value (select value DBNO from CURD.refno where STYP=$db_type) from only MDB where NAME=$mdb limit 1;
    let $a = (select value id from (select REFNO.id as id, array::find_index($dbnos, REFNO.dbnum) as o from WORL where REFNO.dbnum in $dbnos order by o));
    let $sites = array::flatten(select value in.* from $a<-pe_owner[? in.noun='SITE']);
    let $world = $sites[0].owner;

    for $s in $sites{
        relate $s->mdb_owner->$world;
    };
};

