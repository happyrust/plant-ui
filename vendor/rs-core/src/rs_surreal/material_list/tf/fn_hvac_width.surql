
remove function fn::hvac_width_thre;
DEFINE FUNCTION fn::hvac_width_thre($pe:record) {
    let $desp = $pe.refno.DESP;
    let $width_1 = <string>math::fixed($desp[1]?:0,2);
    let $width_2 = <string>math::fixed($desp[3]?:0,2);
    let $width_3 = <string>math::fixed($desp[40]?:0,2);

    return if fn::hvac_shape($this.id) == 'CIRC' {
        return string::concat( string::concat('φ',$width_1) ,',',string::concat('φ',$width_2),',',string::concat('φ',$width_3) );
    } else {
        return string::concat($width_1,',',$width_2,',',$width_3);
    }
};

remove function fn::hvac_height_thre;
DEFINE FUNCTION fn::hvac_height_thre($pe:record) {
    let $desp = $pe.refno.DESP;
    let $height_1 = <string>math::fixed($desp[2]?:0,2);
    let $height_2 = <string>math::fixed($desp[4]?:0,2);
    let $height_3 = <string>math::fixed($desp[41]?:0,2);

    return if fn::hvac_shape($this.id) == 'CIRC' {
        return '';
    } else {
        return string::concat($height_1,',',$height_2,',',$height_3);
    }
};

remove function fn::hvac_width_trns;
DEFINE FUNCTION fn::hvac_width_trns($pe:record) {
    let $desp = $pe.refno.DESP;
    let $height_1 = if $desp[2] == 788296 { 'DIAM' } else { '' };
    let $height_2 = if $desp[4] == 788296 { 'DIAM' } else { '' };
    return if $height_2 == 'DIAM' {
        return string::concat(<string>math::fixed($desp[1]?:0.0,2),',',string::concat('φ',<string>math::fixed($desp[3]?:0.0 , 2)));
    } else if $height == 'DIAM' {
        return string::concat(string::concat('φ',<string>math::fixed($desp[3]?:0.0 , 2)),',',<string>math::fixed($desp[1]?:0.0,2));
    } else { return '' }
};

remove function fn::hvac_height_trns;
DEFINE FUNCTION fn::hvac_height_trns($pe:record) {
    let $desp = $pe.refno.DESP;
    let $height_1 = if $desp[2] == 788296 { 'DIAM' } else { '' };
    let $height_2 = if $desp[4] == 788296 { 'DIAM' } else { '' };
    return if $height_2 == 'DIAM' {
        return string::concat(<string>math::fixed($desp[2]?:0.0,2),',','-');
    } else if $height == 'DIAM' {
        return string::concat('-',',',<string>math::fixed($desp[1]?:0.0,2));
    } else { return '' }
};