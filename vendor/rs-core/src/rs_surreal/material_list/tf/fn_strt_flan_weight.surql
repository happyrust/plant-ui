remove function fn::strt_flan_weight_1;
DEFINE FUNCTION fn::strt_flan_weight_1($pe:record) {
    let $flan_size = fn::strt_flan_size($pe)[0].size?:'';
    let $flan_length = fn::strt_flan_length_1($pe)?:0.0;

    return if $flan_size == 'L40x4' {
        return math::fixed($flan_length * 2.422,2)
    } else if $flan_size == 'L50x5' {
       return math::fixed($flan_length * 3.77,2)
    } else if $flan_size == 'L63x6' {
       return math::fixed($flan_length * 5.721,2)
    } else if $flan_size == 'L90x56x5' {
       return math::fixed($flan_length * 5.661,2)
    } else if $flan_size == 'FB40x5' {
       return math::fixed($flan_length * 1.57,2)
    } else if $flan_size == 'FB50x5' {
       return math::fixed($flan_length * 1.96,2)
    } else if $flan_size == 'FB60x8' {
        return math::fixed($flan_length * 3.768,2)
    } else if $flan_size == 'FB80x8' {
        return math::fixed($flan_length * 5.024,2)
    } else if $flan_size == 'FB80x10' {
        return math::fixed($flan_length * 6.28,2)
    } else if $flan_size == 'FB90x8' {
        return math::fixed($flan_length * 5.652,2)
    } else {
        return ''
    };
};

remove function fn::strt_flan_weight_2;
DEFINE FUNCTION fn::strt_flan_weight_2($pe:record) {
    let $flan_size = fn::strt_flan_size_2($pe)[0].size?:'';
    let $flan_length = fn::strt_flan_length_2($pe)?:0.0;

    let $desp = $pe.refno.DESP;
    let $fl_fls = $desp[67];
    let $fl_fls_2 = $desp[68];

    return
    if $fl_fls != $fl_fls_2 {
        return if $flan_size == 'L40x4' {
            return math::fixed($flan_length * 2.422,2)
        } else if $flan_size == 'L50x5' {
           return math::fixed($flan_length * 3.77,2)
        } else if $flan_size == 'L63x6' {
           return math::fixed($flan_length * 5.721,2)
        } else if $flan_size == 'L90x56x5' {
           return math::fixed($flan_length * 5.661,2)
        } else if $flan_size == 'FB40x5' {
           return math::fixed($flan_length * 1.57,2)
        } else if $flan_size == 'FB50x5' {
           return math::fixed($flan_length * 1.96,2)
        } else if $flan_size == 'FB60x8' {
            return math::fixed($flan_length * 3.768,2)
        } else if $flan_size == 'FB80x8' {
            return math::fixed($flan_length * 5.024,2)
        } else if $flan_size == 'FB80x10' {
            return math::fixed($flan_length * 6.28,2)
        } else if $flan_size == 'FB90x8' {
            return math::fixed($flan_length * 5.652,2)
        }
    } else {
        return '';
    }
};


// 法兰重量
remove function fn::strt_flan_weight;
DEFINE FUNCTION fn::strt_flan_weight($pe:record) {
   return string::concat(<string>(fn::strt_flan_weight_1($pe)?:''),',',<string>(fn::strt_flan_weight_2($pe)?:''));
};

remove function fn::strt_flan_weight_trns_1;
DEFINE FUNCTION fn::strt_flan_weight_trns_1($pe:record) {
    let $flan_size = fn::strt_flan_size($pe)[0].size?:'';
    let $flan_length = fn::strt_flan_length_trns_1($pe)?:0.0;

    return if $flan_size == 'L40x4' {
        return math::fixed($flan_length * 2.422,2)
    } else if $flan_size == 'L50x5' {
       return math::fixed($flan_length * 3.77,2)
    } else if $flan_size == 'L63x6' {
       return math::fixed($flan_length * 5.721,2)
    } else if $flan_size == 'L90x56x5' {
       return math::fixed($flan_length * 5.661,2)
    } else if $flan_size == 'FB40x5' {
       return math::fixed($flan_length * 1.57,2)
    } else if $flan_size == 'FB50x5' {
       return math::fixed($flan_length * 1.96,2)
    } else if $flan_size == 'FB60x8' {
        return math::fixed($flan_length * 3.768,2)
    } else if $flan_size == 'FB80x8' {
        return math::fixed($flan_length * 5.024,2)
    } else if $flan_size == 'FB80x10' {
        return math::fixed($flan_length * 6.28,2)
    } else if $flan_size == 'FB90x8' {
        return math::fixed($flan_length * 5.652,2)
    } else {
        return ''
    };
};

remove function fn::strt_flan_weight_trns_2;
DEFINE FUNCTION fn::strt_flan_weight_trns_2($pe:record) {
    let $flan_size = fn::strt_flan_size_2($pe)[0].size?:'';
    let $flan_length = fn::strt_flan_length_trns_2($pe)?:0.0;

    let $desp = $pe.refno.DESP;
    let $fl_fls = $desp[67];
    let $fl_fls_2 = $desp[68];

    return
    if $fl_fls != $fl_fls_2 {
        return if $flan_size == 'L40x4' {
            return math::fixed($flan_length * 2.422,2)
        } else if $flan_size == 'L50x5' {
           return math::fixed($flan_length * 3.77,2)
        } else if $flan_size == 'L63x6' {
           return math::fixed($flan_length * 5.721,2)
        } else if $flan_size == 'L90x56x5' {
           return math::fixed($flan_length * 5.661,2)
        } else if $flan_size == 'FB40x5' {
           return math::fixed($flan_length * 1.57,2)
        } else if $flan_size == 'FB50x5' {
           return math::fixed($flan_length * 1.96,2)
        } else if $flan_size == 'FB60x8' {
            return math::fixed($flan_length * 3.768,2)
        } else if $flan_size == 'FB80x8' {
            return math::fixed($flan_length * 5.024,2)
        } else if $flan_size == 'FB80x10' {
            return math::fixed($flan_length * 6.28,2)
        } else if $flan_size == 'FB90x8' {
            return math::fixed($flan_length * 5.652,2)
        }
    } else {
        return '';
    }
};

// 法兰重量
remove function fn::strt_flan_weight_trns;
DEFINE FUNCTION fn::strt_flan_weight_trns($pe:record) {
   return string::concat(<string>(fn::strt_flan_weight_trns_1($pe)?:''),',',<string>(fn::strt_flan_weight_trns_2($pe)?:''));
};

remove function fn::strt_flan_weight_thre_1;
DEFINE FUNCTION fn::strt_flan_weight_thre_1($pe:record) {
    let $flan_size = fn::strt_flan_size($pe)[0].size?:'';
    let $flan_length = fn::strt_flan_length_thre_1($pe)?:0.0;

    return if $flan_size == 'L40x4' {
        return math::fixed($flan_length * 2.422,2)
    } else if $flan_size == 'L50x5' {
       return math::fixed($flan_length * 3.77,2)
    } else if $flan_size == 'L63x6' {
       return math::fixed($flan_length * 5.721,2)
    } else if $flan_size == 'L90x56x5' {
       return math::fixed($flan_length * 5.661,2)
    } else if $flan_size == 'FB40x5' {
       return math::fixed($flan_length * 1.57,2)
    } else if $flan_size == 'FB50x5' {
       return math::fixed($flan_length * 1.96,2)
    } else if $flan_size == 'FB60x8' {
        return math::fixed($flan_length * 3.768,2)
    } else if $flan_size == 'FB80x8' {
        return math::fixed($flan_length * 5.024,2)
    } else if $flan_size == 'FB80x10' {
        return math::fixed($flan_length * 6.28,2)
    } else if $flan_size == 'FB90x8' {
        return math::fixed($flan_length * 5.652,2)
    } else {
        return ''
    };
};

remove function fn::strt_flan_weight_thre_2;
DEFINE FUNCTION fn::strt_flan_weight_thre_2($pe:record) {
    let $flan_size = fn::strt_flan_size_2($pe)[0].size?:'';
    let $flan_length = fn::strt_flan_length_thre_2($pe)?:0.0;

    let $desp = $pe.refno.DESP;
    let $fl_fls = $desp[67];
    let $fl_fls_2 = $desp[68];

    return
    if $fl_fls != $fl_fls_2 {
        return if $flan_size == 'L40x4' {
            return math::fixed($flan_length * 2.422,2)
        } else if $flan_size == 'L50x5' {
           return math::fixed($flan_length * 3.77,2)
        } else if $flan_size == 'L63x6' {
           return math::fixed($flan_length * 5.721,2)
        } else if $flan_size == 'L90x56x5' {
           return math::fixed($flan_length * 5.661,2)
        } else if $flan_size == 'FB40x5' {
           return math::fixed($flan_length * 1.57,2)
        } else if $flan_size == 'FB50x5' {
           return math::fixed($flan_length * 1.96,2)
        } else if $flan_size == 'FB60x8' {
            return math::fixed($flan_length * 3.768,2)
        } else if $flan_size == 'FB80x8' {
            return math::fixed($flan_length * 5.024,2)
        } else if $flan_size == 'FB80x10' {
            return math::fixed($flan_length * 6.28,2)
        } else if $flan_size == 'FB90x8' {
            return math::fixed($flan_length * 5.652,2)
        }
    } else {
        return '';
    }
};

// 法兰重量
remove function fn::strt_flan_weight_thre;
DEFINE FUNCTION fn::strt_flan_weight_thre($pe:record) {
   return string::concat(<string>(fn::strt_flan_weight_thre_1($pe)?:''),',',<string>(fn::strt_flan_weight_thre_2($pe)?:''));
};
