// 判断bran 按 ‘-’分割的第3位是否等于‘6’
remove function fn::bran_3_b_6;
DEFINE FUNCTION fn::bran_3_b_6($pe:record) {
    return array::at(string::split($pe.owner.name,'-'),3)?:'' == '6'
};