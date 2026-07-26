
insert ignore into hvac_mat [{
    key: '1',
    code: 'SS',
    name: '不锈钢'
}, {
    key: 2,
    code: 'GS',
    name: '镀锌钢（非气密）'
},
{
    key: 3,
    code: 'CS',
    name: '碳钢（非气密）'
},
{
    key: 4,
    code: 'ACS',
    name: '气密性碳钢'
},
{
    key: 5,
    code: 'ASS',
    name: '气密性不锈钢'
},
{
    key: 7,
    code: 'AS',
},
{
    key: 8,
    code: 'FP',
}]

insert ignore into hvac_pressure [{
    id: 'B',
    code: 'BP',
    name: '低压' 
},
{
    id: 'M',
    code: 'MP',
    name: '中压' 
},
{
    id: 'H',
    code: 'HP',
    name: '高压' 
},
{
    id: 'T',
    code: 'TP',
    name: '超高压' 
}]

//shape * 8 + mat
insert ignore into hvac_depth [{
    // 矩形镀锌 中低压  L<= 700
    id: '1',
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [0, 700],
    depth: 0.8,
    hvac_size: '无' // 加强筋规格
},{
    // 矩形镀锌 中低压 700 < L <= 1400
    id: '2',
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [700, 1400],
    depth: 1.0,
},{
    // 矩形镀锌 中低压 1400 < L
    id: '3'
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [1400, 100000],
    depth: 1.2
},
{
    // 矩形镀锌 高压 L <= 700
    id: '4',
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:H],
    max_length: [0, 700],
    depth: 1.0
}, {
    // 矩形镀锌 高压 700 < L
    id: '5',
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:H],
    max_length: [700, 100000],
    depth: 1.2
},{
    // 矩形非气密性不锈钢 中低高压 所有
    id: '6',
    mat: 1,
    type: 1,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
    max_length: [0, 100000],
    depth: 1.2
},
{
    // 矩形气密性不锈钢 中低高压 所有
    id: '7',
    mat: 1,
    type: 5,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
    max_length: [0, 100000],
    depth: 2
},{
      // 矩形气密性不锈钢 超高压 所有
      id: '8',
      mat: 1,
      type: 5,
      b_indoor: true,
      pressure: [hvac_pressure:T],
      max_length: [0, 100000],
      depth: 0
},{
      // 矩形非气密性碳钢 中低高压 所有
      id: '9',
      mat: 1,
      type: 3,
      b_indoor: true,
      pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
      max_length: [0, 100000],
      depth: 1.5
},{
     // 矩形气密性碳钢 中低高压 L<=1400
      id: '10',
      mat: 1,
      type: 4,
      b_indoor: true,
      pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
      max_length: [0, 1400],
      depth: 1.5
},{
       // 矩形气密性碳钢 中低高压 L > 1400
       id: '11',
       mat: 1,
       type: 4,
       b_indoor: true,
       pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
       max_length: [1400,100000],
       depth: 2
},{
       // 矩形气密性碳钢 超高压 所有
       id: '12',
       mat: 1,
       type: 4,
       b_indoor: true,
       pressure: [hvac_pressure:T],
       max_length: [0, 100000],
       depth: 0
},{
        // 室外矩形非气密性碳钢 所有 所有
        id: '13',
        mat: 1,
        type: 3,
        b_indoor:false,
        pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
        max_length: [0, 100000],
        depth: 3
},{
        // 室外矩形气密性碳钢 所有 所有
        id: '14',
        mat: 1,
        type: 4,
        b_indoor:false,
        pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
        max_length: [0, 100000],
        depth: 3
},{
         // 圆形非气密性碳钢 所有 所有
         id: '15',
         mat: 2,
         type: 3,
         b_indoor: true,
         pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
         max_length: [0, 100000],
         depth: 1.5
},{
          // 圆形气密性碳钢 所有 所有
          id: '16',
          mat: 2,
          type: 4,
          b_indoor: true,
          pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
          max_length: [0, 100000],
          depth: 1.5
},{
          // 圆形非气密性不锈钢 所有 所有
          id: '17',
          mat: 2,
          type: 1,
          b_indoor: true,
          pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
          max_length: [0, 100000],
          depth: 2
},{
          // 圆形气密性不锈钢 所有 所有
          id: '18',
          mat: 2,
          type: 5,
          b_indoor: true,
          pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
          max_length: [0, 100000],
          depth: 2
},{
          // 室外圆形非气密性碳钢
          id: '19',
          mat: 2,
          type: 3,
          b_indoor:false,
          pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
          max_length: [0, 100000],
          depth: 3
},{
           // 室外圆形气密性碳钢
           id: '20',
           mat: 2,
           type: 4,
           b_indoor:false,
           pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
           max_length: [0, 100000],
           depth: 3
 }
]

// 法兰、加强截面积和法兰宽通过型钢规格判断
insert ignore into hvac_area [{
    id: '1',
    size: 'L40x40x4',
    area: 3.086,
    flan_width: 0.04
},{
    id: '2',
    size: 'L50x50x5',
    area: 4.803,
    flan_width: 0.05
},{
   id: '3',
   size: 'L60x60x6',
   area: 6.914,
   flan_width: 0.06
},{
   id: '4',
   size: 'FB40x5',
   area: 2,
   flan_width: 0.04
},{
   id: '5',
   size: 'FB50x5',
   area: 2.5,
   flan_width: 0.05
},{
    id: '6',
    size: 'FB60x8',
    area: 4.8,
    flan_width: 0.06
}]


// 加强筋规格
insert ignore into hvac_size [{
    // 矩形镀锌 中低压  L<= 700
    id: '1',
    mat: 1,
    type: 2,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [0, 700],
    hvac_size: '' // 加强筋规格
},{
    // 矩形镀锌 中低压 700 < L <= 1400
    id: '2',
    mat: 1,
    type: 2,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [700, 1400],
    hvac_size: hvac_area:L40x40x4
},{
    // 矩形镀锌 中低压 1400 < L
    id: '3',
    mat: 1,
    type: 2,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [1400, 100000],
    hvac_size: hvac_area:L40x40x4
},
{
    // 矩形镀锌 高压 L <= 700
    id: '4',
    mat: 1,
    type: 2,
    pressure: [hvac_pressure:H],
    max_length: [0, 700],
    hvac_size: hvac_area:L40x40x4
}, {
    // 矩形镀锌 高压 700 < L <= 1400
    id: '5',
    mat: 1,
    type: 2,
    pressure: [hvac_pressure:H],
    max_length: [700, 1400],
    hvac_size: hvac_area:L40x40x4
},{
    // 矩形镀锌 高压 1400 < L
    id: '6',
    mat: 1,
    type: 2,
    pressure: [hvac_pressure:H],
    max_length: [1400,100000],
    hvac_size: hvac_area:L50x50x5
},{
    // 矩形非气密性不锈钢 中低压 L<=700
    id: '7',
    mat: 1,
    type: 1,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [0, 700],
    hvac_size: ''
},
{
    // 矩形非气密性不锈钢 中低压 700<L<=1400
    id: '8',
    mat: 1,
    type: 1,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [700, 1400],
    hvac_size: hvac_area:L40x40x4
},{
    // 矩形非气密性不锈钢 高压 0<=L<=1400
    id: '9',
    mat: 1,
    type: 1,
    pressure: [hvac_pressure:H],
    max_length: [0, 1400],
    hvac_size: hvac_area:L40x40x4
},{
     // 矩形非气密性不锈钢 高 L > 1400
     id: '10',
     mat: 1,
     type: 1,
     pressure: [hvac_pressure:H],
     max_length: [1400,100000],
     hvac_size: hvac_area:L50x50x5
},{
     // 矩形气密性不锈钢 中低压 0 <= L <=1400
     id: '11',
     mat: 1,
     type: 5,
     pressure: [hvac_pressure:B, hvac_pressure:M],
     max_length: [0, 1400],
     hvac_size: ''
},{
      // 矩形气密性不锈钢 中低压 1400<L
      id: '12',
      mat: 1,
      type: 5,
      pressure: [hvac_pressure:B, hvac_pressure:M],
      max_length: [1400,100000],
      hvac_size: hvac_area:FB40x5
},{
      // 矩形气密性不锈钢 高压 L<=700
      id: '13',
      mat: 1,
      type: 5,
      pressure: [hvac_pressure:H],
      max_length: [0,700],
      hvac_size: hvac_area:FB40x5
},{
      // 矩形气密性不锈钢 高压 700 < L <= 1400
      id: '14',
      mat: 1,
      type: 5,
      pressure: [hvac_pressure:H],
      max_length: [700,1400],
      hvac_size: hvac_area:FB50x5
},{
      // 矩形气密性不锈钢 高压 1400 < L
      id: '15',
      mat: 1,
      type: 5,
      pressure: [hvac_pressure:H],
      max_length: [1400,100000],
      hvac_size: hvac_area:FB60x8
},{
      // 矩形气密性碳钢 中低压 0<= L <= 1400
      id: '16',
      mat: 1,
      type: 4,
      pressure: [hvac_pressure:B, hvac_pressure:M],
      max_length: [0, 1400],
      hvac_size: ''
},{
      // 矩形气密性碳钢 中低压 1400<= L
      id: '17',
      mat: 1,
      type: 4,
      pressure: [hvac_pressure:B, hvac_pressure:M],
      max_length: [1400,100000],
      hvac_size: hvac_area:FB40x5
},{
      // 矩形气密性碳钢 高压 L <= 700
      id: '18',
      mat: 1,
      type: 4,
      pressure: [hvac_pressure:H],
      max_length: [0,700],
      hvac_size: hvac_area:FB40x5
},{
      // 矩形气密性碳钢 高压 700 < L <= 1400
      id: '19',
      mat: 1,
      type: 4,
      pressure: [hvac_pressure:H],
      max_length: [700,1400],
      hvac_size: hvac_area:FB50x5
},{
       // 矩形气密性碳钢 高压 L > 1400
       id: '20',
       mat: 1,
       type: 4,
       pressure: [hvac_pressure:H],
       max_length: [1400,100000],
       hvac_size: hvac_area:FB60x8
  },{
       // 矩形非气密性碳钢 中低压 0<= L <= 1400
       id: '21',
       mat: 1,
       type: 3,
       pressure: [hvac_pressure:B, hvac_pressure:M],
       max_length: [0, 1400],
       hvac_size: ''
 },{
       // 矩形非气密性碳钢 中低压 1400<= L
       id: '22',
       mat: 1,
       type: 3,
       pressure: [hvac_pressure:B, hvac_pressure:M],
       max_length: [1400,100000],
       hvac_size: hvac_area:FB40x5
   },{
       // 矩形非气密性碳钢 高压 L <= 700
       id: '23',
       mat: 1,
       type: 3,
       pressure: [hvac_pressure:H],
       max_length: [0,700],
       hvac_size: hvac_area:FB40x5
   },{
       // 矩形非气密性碳钢 高压 700 < L <= 1400
       id: '24',
       mat: 1,
       type: 3,
       pressure: [hvac_pressure:H],
       max_length: [700,1400],
       hvac_size: hvac_area:FB50x5
   },{
       // 矩形非气密性碳钢 高压 L > 1400
       id: '20',
       mat: 1,
       type: 3,
       pressure: [hvac_pressure:H],
       max_length: [1400,100000],
       hvac_size: hvac_area:FB60x8
     }
]

-- delete hvac_depth;

insert ignore into hvac_flan_size [{
    // 矩形镀锌 中低压  L<= 700
    id: '1',
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [0, 700],
    hvac_size: hvac_area:L40x40x4 // 法兰规格
},{
    // 矩形镀锌 中低压 700 < L
    id: '2',
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [700,1000000],
    hvac_size: hvac_area:L50x50x5
},{
    // 矩形镀锌 高压 L <= 1400
    id: '3',
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:H],
    max_length: [0, 1400],
    hvac_size: hvac_area:L50x50x5
}, {
    // 矩形镀锌 高压 1400 < L
    id: '4',
    mat: 1,
    type: 2,
    b_indoor: true,
    pressure: [hvac_pressure:H],
    max_length: [1400,100000],
    hvac_size: hvac_area:L60x60x6
},{
    // 矩形非气密性不锈钢 中低压 L<=700
    id: '5',
    mat: 1,
    type: 1,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [0, 700],
    hvac_size: hvac_area:L40x40x4
},
{
    // 矩形非气密性不锈钢 中低压 700<L
    id: '6',
    mat: 1,
    type: 1,
    b_indoor: true,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [700, 1000000],
    hvac_size: hvac_area:L50x50x5
},{
    // 矩形非气密性不锈钢 高压 0<=L<=1400
    id: '7',
    mat: 1,
    type: 1,
    b_indoor: true,
    pressure: [hvac_pressure:H],
    max_length: [0, 1400],
    hvac_size: hvac_area:L50x50x5
},{
     // 矩形非气密性不锈钢 高 L > 1400
     id: '8',
     mat: 1,
     type: 1,
     b_indoor: true,
     pressure: [hvac_pressure:H],
     max_length: [1400,100000],
     hvac_size: hvac_area:L60x60x6
},{
     // 矩形气密性不锈钢 中低高压 0 <= L <= 700
     id: '9',
     mat: 1,
     type: 5,
     b_indoor: true,
     pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
     max_length: [0, 700],
     hvac_size: hvac_area:FB40x5
},{
      // 矩形气密性不锈钢 中低高压 700<L<=1400
      id: '10',
      mat: 1,
      type: 5,
      b_indoor: true,
      pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
      max_length: [700,1400],
      hvac_size: hvac_area:FB50x5
},{
      // 矩形气密性不锈钢 高压 1400 < L
      id: '11',
      mat: 1,
      type: 5,
      b_indoor: true,
      pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
      max_length: [1400,1000000],
      hvac_size: hvac_area:FB60x8
},{
      // 圆形气密性碳钢 所有  L <= 700
      id: '12',
      mat: 2,
      type: 4,
      b_indoor: true,
      pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
      max_length: [0, 700],
      hvac_size: hvac_area:FB40x5
},{
      // 圆形气密性碳钢 所有  700 < L <= 1400
      id: '13',
      mat: 2,
      type: 4,
      b_indoor: true,
      pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
      max_length: [700, 1400],
      hvac_size: hvac_area:FB50x5
},{
      // 圆形气密性碳钢 所有  1400 < L
      id: '14',
      mat: 2,
      type: 4,
      b_indoor: true,
      pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
      max_length: [1400,1000000],
      hvac_size: hvac_area:FB60x8
},{
       // 圆形非气密性碳钢 所有  L <= 700
       id: '15',
       mat: 2,
       type: 3,
       b_indoor: true,
       pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
       max_length: [0, 700],
       hvac_size: hvac_area:FB40x5
 },{
       // 圆形非气密性碳钢 所有  700 < L <= 1400
       id: '16',
       mat: 2,
       type: 3,
       b_indoor: true,
       pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
       max_length: [700, 1400],
       hvac_size: hvac_area:FB50x5
 },{
       // 圆形非气密性碳钢 所有  1400 < L
       id: '17',
       mat: 2,
       type: 3,
       b_indoor: true,
       pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
       max_length: [1400,1000000],
       hvac_size: hvac_area:FB60x8
 },{
         // 圆形非气密性碳钢 所有  L <= 700
         id: '18',
         mat: 2,
         type: 3,
         b_indoor: true,
         pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
         max_length: [0, 700],
         hvac_size: hvac_area:FB40x5
   },{
         // 圆形非气密性碳钢 所有  700 < L <= 1400
         id: '19',
         mat: 2,
         type: 3,
         b_indoor: true,
         pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
         max_length: [700, 1400],
         hvac_size: hvac_area:FB50x5
   },{
         // 圆形非气密性碳钢 所有  1400 < L
         id: '20',
         mat: 2,
         type: 3,
         b_indoor: true,
         pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
         max_length: [1400,1000000],
         hvac_size: hvac_area:FB60x8
   },{
       // 圆形气密性不锈钢 所有  L <= 700
       id: '21',
       mat: 2,
       type: 5,
       b_indoor: true,
       pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
       max_length: [0, 700],
       hvac_size: hvac_area:FB40x5
 },{
       // 圆形气密性不锈钢 所有  700 < L <= 1400
       id: '22',
       mat: 2,
       type: 5,
       b_indoor: true,
       pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
       max_length: [700, 1400],
       hvac_size: hvac_area:FB50x5
 },{
       // 圆形气密性不锈钢 所有  1400 < L
       id: '23',
       mat: 2,
       type: 5,
       b_indoor: true,
       pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
       max_length: [1400,1000000],
       hvac_size: hvac_area:FB60x8
 },{
        // 圆形非气密性不锈钢 所有  L <= 700
        id: '24',
        mat: 2,
        type: 1,
        b_indoor: true,
        pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
        max_length: [0, 700],
        hvac_size: hvac_area:FB40x5
  },{
        // 圆形气密性不锈钢 所有  700 < L <= 1400
        id: '25',
        mat: 2,
        type: 1,
        b_indoor: true,
        pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
        max_length: [700, 1400],
        hvac_size: hvac_area:FB50x5
  },{
        // 圆形气密性不锈钢 所有  1400 < L
        id: '26',
        mat: 2,
        type: 1,
        b_indoor: true,
        pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H,hvac_pressure:T],
        max_length: [1400,1000000],
        hvac_size: hvac_area:FB60x8
  },{
        // 室外气密性碳钢 所有 所有
        id: '27',
        mat: 2,
        type: 4,
        b_indoor: false,
        pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
        max_length: [0,1000000],
        hvac_size: hvac_area:FB60x8
  },{
        // 室外非气密性碳钢 所有 所有
        id: '28',
        mat: 2,
        type: 3,
        b_indoor: false,
        pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
        max_length: [0,1000000],
        hvac_size: hvac_area:FB60x8
}
]

//8 * shape + mat

let $type = 1;
let $max_len = 800;
let $pressure = 'B';

select * from only hvac_depth where type=$type and type::thing("hvac_pressure", $pressure) in pressure and $max_len > max_length[0] and $max_len <= max_length[1] limit 1


-- select * from hvac_mat


