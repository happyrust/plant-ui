
remove function fn::collect_hvac_SITE;
DEFINE FUNCTION fn::collect_hvac_SITE($pe: record) {
    let $children = select value in from $pe<-pe_owner.in<-pe_owner[?in.noun='HVAC'].in<-pe_owner[?in.noun='BRAN'];
    return fn::collect_hvac_brans($children);
};

remove function fn::collect_hvac_ZONE;
DEFINE FUNCTION fn::collect_hvac_ZONE($pe: record) {
    let $children = select value in from $pe<-pe_owner[?in.noun='HVAC'].in<-pe_owner[?in.noun='BRAN'];
    return fn::collect_hvac_brans($children);
};

remove function fn::collect_hvac;
DEFINE FUNCTION fn::collect_hvac($pe: record) {
    let $children = fn::children($pe);
    return fn::collect_hvac_brans($children);
};

remove function fn::collect_hvac_ele;
DEFINE FUNCTION fn::collect_hvac_ele($pe: record) {
    let $t = $pe.noun;
    return if $t =='STRT' {
       return fn::collect_STRT([$pe])[0];
    } else if $t == 'OFST' {
       return fn::collect_OFST([$pe])[0];
    } else if $t == 'STIF' {
       return fn::collect_STIF([$pe])[0];
    } else if $t == 'TAPE' {
       return fn::collect_TAPE([$pe])[0];
    } else if $t == 'TRNS' {
       return fn::collect_TRNS([$pe])[0];
    } else if $t == 'THRE' {
       return fn::collect_THRE([$pe])[0];
    } else if $t == 'BRCO' {
       return fn::collect_BRCO([$pe])[0];
    } else if $t == 'BEND' {
       return fn::collect_BEND([$pe])[0];
    } else if $t == 'CAP' {
       return fn::collect_CAP([$pe])[0];
    } else {
       return;
    };
};

remove function fn::collect_hvac_brans;
DEFINE FUNCTION fn::collect_hvac_brans($pes: array<record>) {
    return array::flatten(select value fn::collect_hvac_bran(id) from $pes);
};

remove function fn::collect_hvac_bran;
DEFINE FUNCTION fn::collect_hvac_bran($pe: record) {
    let $children = fn::children($pe);
    let $r = select value fn::collect_hvac_ele(id) from $children;
    return array::complement($r, [none]);
};