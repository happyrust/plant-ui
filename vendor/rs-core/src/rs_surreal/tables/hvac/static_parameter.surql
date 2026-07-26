
insert ignore into hvac_mat [{
    id: 1,
    code: 'SS',
    name: '不锈钢'
}, {
    id: 2,
    code: 'GS',
    name: '镀锌钢（非气密）'
},
{
    id: 3,
    code: 'CS',
    name: '碳钢（非气密）'
},
{
    id: 4,
    code: 'ACS',
    name: '气密性碳钢'
},
{
    id: 5,
    code: 'ASS',
    name: '气密性不锈钢'
},
{
    id: 7,
    code: 'AS',
},
{
    id: 8,
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
    id: 1,
    type: 2,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [0, 700],
    depth: 0.8
},{
    id: 2,
    type: 2,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [700, 1400],
    depth: 1.0
},{
    id: 3,
    type: 2,
    pressure: [hvac_pressure:B, hvac_pressure:M],
    max_length: [1400, 100000],
    depth: 1.2
},

{
    id: 4,
    type: 2,
    pressure: [hvac_pressure:H],
    max_length: [0, 700],
    depth: 1.0
}, {
    id: 5,
    type: 2,
    pressure: [hvac_pressure:H],
    max_length: [700, 100000],
    depth: 1.2
},

{
    id: 6,
    type: 1,
    pressure: [hvac_pressure:B, hvac_pressure:M, hvac_pressure:H],
    max_length: [0, 100000],
    depth: 1.2
},


]

-- delete hvac_depth;


//8 * shape + mat

let $type = 1;
let $max_len = 800;
let $pressure = 'B';

select * from only hvac_depth where type=$type and type::thing("hvac_pressure", $pressure) in pressure and $max_len > max_length[0] and $max_len <= max_length[1] limit 1


-- select * from hvac_mat


