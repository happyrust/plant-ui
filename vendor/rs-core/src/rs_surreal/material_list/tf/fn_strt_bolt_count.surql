// 螺栓数量
remove function fn::strt_bolt_count;
DEFINE FUNCTION fn::strt_bolt_count($pe:record) {
   let $uda_value = fn::get_uda_value($pe.id, '/BoltStatis')?:'';
   return string::concat(string::split($uda_value,',')[0]?:'' , ',' , string::split($uda_value,',')[2]?:'')
};
