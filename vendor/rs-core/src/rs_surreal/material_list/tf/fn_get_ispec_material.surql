// 材质 2
remove function fn::get_ispec_material;
DEFINE FUNCTION fn::get_ispec_material($pe:record) {
    let $ispec = $pe.refno.ISPE.name;
    let $bran_4 = array::at(string::split($pe.owner.name?:'','-'),3)?:'';
    return
    if ($bran_4 == '1' || $bran_4 == '2' || $bran_4 == '3' || $bran_4 == '4' || $bran_4 == '5') && $ispec != NONE  {
        return '保温材料'
    } else if $bran_4 == '6' && $ispec != NONE {
        return '防火材料'
    } else {
        return ''
    }
};