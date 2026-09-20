# 供数模式对拍报告 · AvevaMarineSample /ALL

> 差异只列不判：服务供数读文件最新、库供数读水位，两边时点可以不同。对时点看下面两枚凭证。

- 生成：2026-09-07 21:20:21 UTC（用时 15.8 s）
- 服务供数：`http://127.0.0.1:8022`
- 库供数：`ws://localhost:8009 ns 1516 / /ALL`
- 参数：`--depth 2` `--sample 200`；三维实例抽样根：从第 1 层共有节点里等距抽 ≤ 10 个

## 凭证（`/dbnums`）

- 服务端数据形态 `data_face = ingest`。`applied_sesno` 是库里应用到的会话号（库供数读的水位），`file_latest_sesno` 是文件此刻自报的最新会话号（服务供数读的时点）。两数不同的库，属性与实例的差异不算缺陷。

| dbnum | 类型 | applied_sesno（库水位） | file_latest_sesno（文件最新） | 差 | 备注 |
|---|---|---:|---:|---:|---|
| 1112 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 5100 | DICT | 27 | 27 | 0 |  |
| 5101 | DICT | 7 | 7 | 0 |  |
| 6003 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7001 |  | 270 | 0 | -270 | 不在本期范围 |
| 7006 | DICT | 17 | 17 | 0 |  |
| 7009 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7011 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7015 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7022 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7027 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7032 | DICT | 57 | 57 | 0 |  |
| 7043 | DICT | 13 | 13 | 0 |  |
| 7048 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7049 | DICT | 4 | 4 | 0 |  |
| 7321 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7323 | DICT | 277 | 277 | 0 |  |
| 7324 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7326 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7327 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7329 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7330 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7331 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7332 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7333 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7334 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7350 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7352 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7353 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7354 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7356 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7741 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 7997 | DESI | 106 | 106 | 0 | 不在本期范围 |
| 7998 | DESI | 0 | 12 | 12 |  |
| 7999 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 8000 | DESI | 323 | 379 | 56 |  |
| 8189 | MISC | 1 | 1 | 0 |  |
| 8190 | COMM | 1 | 1 | 0 |  |
| 8191 | SYST | 180 | 230 | 50 |  |
| 250704 | DICT | 3 | 3 | 0 |  |
| 250705 | DESI | 0 | 0 | 0 | MDB 声明了、项目目录里没有 |
| 251047 | DICT | 6 | 6 | 0 |  |

## 工程标识

| 格 | 服务供数 | 库供数 | 一致 |
|---|---|---|---|
| project | AvevaMarineSample | AvevaMarineSample | 是 |
| mdb | /ALL | /ALL | 是 |
| ns | 1516 | 1516 | 是 |
| 设计库 | 7997, 7998, 8000 | 7997, 8000 | **否** |

## 树

| 层 | 比过的父节点 | 服务 | 库 | 共有 | 只在服务 | 只在库 | 原序不一致的父节点 | 查询失败 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 0（SITE 根层） | 1 | 37 | 16 | 16 | 21 | 0 | 0 | 0 |
| 1 | 16 | 151 | 151 | 151 | 0 | 0 | 0 | 0 |
| 2 | 151 | 2548 | 2548 | 2548 | 0 | 0 | 0 | 0 |

roots 集合或原序**有差异**（硬标准没过；两边库的数据本身是否同源先看凭证与设计库那一行）。

### 第 0 层差异

- 只在服务：SITE /1RS-CIVI (=9304/2) ← MDB 世界
- 只在服务：SITE /1WCC-PIPEBJ (=24383/66456) ← MDB 世界
- 只在服务：SITE /1RCV-PIPEBJ (=24383/73927) ← MDB 世界
- 只在服务：SITE /1AXIS (=24383/80151) ← MDB 世界
- 只在服务：SITE /1RX-ELECBJ (=24383/80211) ← MDB 世界
- 只在服务：SITE /Steel_Template_Site (=15201/535) ← MDB 世界
- 只在服务：SITE /MDS/SPECIALS (=23716/1) ← MDB 世界
- 只在服务：SITE /MDS/SPECIALS (=23714/1) ← MDB 世界
- 只在服务：SITE /MDS/SPECIALS (=23715/1) ← MDB 世界
- 只在服务：SITE /MDS-Standards-Site (=23705/1) ← MDB 世界
- 只在服务：SITE /MDS-Standards-Supports (=23705/289) ← MDB 世界
- 只在服务：SITE /MDS/TEMPLATES (=23713/2343) ← MDB 世界
- 只在服务：SITE /MDS/TEMPLATES/ORI (=23737/1) ← MDB 世界
- 只在服务：SITE /MDS/TEMPLATES (=15516/2) ← MDB 世界
- 只在服务：SITE /MDS/TEMPLATES/ORI (=23736/1) ← MDB 世界
- 只在服务：SITE /MDS/TEMPLATES (=23734/1) ← MDB 世界
- 只在服务：SITE /MDS/TEMPLATES/ORI (=23738/1) ← MDB 世界
- 只在服务：SITE /MDS/HANGERS (=23711/1) ← MDB 世界
- 只在服务：SITE /MDS/TEMPLATES (=23740/1) ← MDB 世界
- 只在服务：SITE /MDS/HANGERS (=23717/2) ← MDB 世界
- 只在服务：SITE /MDS/HANGERS (=23710/1) ← MDB 世界

## 属性

抽样 200 个两边都有的元素：逐字段比了 200 个；库供数空表 0 个（元件库元素或未同步，不猜是哪种）；服务空表 0 个；两边都空 0 个；查询失败 0 次。

| 档 | 字段数 |
|---|---:|
| 相同 | 1227 |
| 只差首尾空白 | 1 |
| 只差括号与空格 | 0 |
| 不同 | 1756 |
| 只在服务有的字段 | 12815 |
| 只在库有的字段 | 3602 |

### 「不同」按字段

同一个字段在抽样里几乎每个元素都不同，多半是两面写法不一样（refno 的分隔符、方位串、未设值的写法），不是数据不一样；只落在个别元素上的才值得对着凭证看。

| 字段 | 不同的元素数 |
|---|---:|
| `OWNER` | 200 |
| `REFNO` | 200 |
| `AREA` | 164 |
| `ORI` | 155 |
| `POS` | 155 |
| `NUMB` | 151 |
| `ISPE` | 125 |
| `PSPE` | 47 |
| `TSPE` | 47 |
| `CASR` | 36 |
| `CCEN` | 36 |
| `CCLA` | 36 |
| `DRRF` | 36 |
| `EREC` | 36 |
| `FLUR` | 36 |
| `JMAX` | 36 |
| `MATR` | 36 |
| `PMAX` | 36 |
| `SAFC` | 36 |
| `SMAX` | 36 |
| `WMAX` | 36 |
| `REV` | 34 |
| `SLOREF` | 26 |
| `DESC` | 11 |
| `HSPE` | 9 |

### 「不同」明细（前 50 条）

| 元素 | 字段 | 服务供数 | 库供数 |
|---|---|---|---|
| SITE /1RB-CIVI (=24381/42520) | AREA | `unset` | `0` |
| SITE /1RB-CIVI (=24381/42520) | DESC | `外层安全壳` | `unset` |
| SITE /1RB-CIVI (=24381/42520) | NUMB | `unset` | `0` |
| SITE /1RB-CIVI (=24381/42520) | ORI | `0, 0, 0` | `Y is N and Z is U` |
| SITE /1RB-CIVI (=24381/42520) | OWNER | `16189/0` | `/*` |
| SITE /1RB-CIVI (=24381/42520) | POS | `0, 0, 0` | `X 0mm, Y 0mm, Z 0mm` |
| SITE /1RB-CIVI (=24381/42520) | REFNO | `24381/42520` | `24381_42520` |
| SITE /1RX03-PIPEBJ (=24384/46) | AREA | `unset` | `0` |
| SITE /1RX03-PIPEBJ (=24384/46) | NUMB | `unset` | `0` |
| SITE /1RX03-PIPEBJ (=24384/46) | ORI | `0, 0, 0` | `Y is N and Z is U` |
| SITE /1RX03-PIPEBJ (=24384/46) | OWNER | `16192/0` | `/*` |
| SITE /1RX03-PIPEBJ (=24384/46) | POS | `0, 0, 0` | `X 0mm, Y 0mm, Z 0mm` |
| SITE /1RX03-PIPEBJ (=24384/46) | REFNO | `24384/46` | `24384_46` |
| ZONE /暖通专业SL (=24381/15342) | AREA | `unset` | `0` |
| ZONE /暖通专业SL (=24381/15342) | ISPE | `0/0` | `unset` |
| ZONE /暖通专业SL (=24381/15342) | NUMB | `unset` | `0` |
| ZONE /暖通专业SL (=24381/15342) | ORI | `0, 0, 0` | `Y is N and Z is U` |
| ZONE /暖通专业SL (=24381/15342) | OWNER | `24381/776` | `/SSC测试` |
| ZONE /暖通专业SL (=24381/15342) | POS | `0, 0, 0` | `X 0mm, Y 0mm, Z 0mm` |
| ZONE /暖通专业SL (=24381/15342) | PSPE | `0/0` | `unset` |
| ZONE /暖通专业SL (=24381/15342) | REFNO | `24381/15342` | `24381_15342` |
| ZONE /暖通专业SL (=24381/15342) | TSPE | `0/0` | `unset` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | AREA | `unset` | `0` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | DESC | `环形空间通风系统` | `unset` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | ISPE | `0/0` | `unset` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | NUMB | `unset` | `0` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | ORI | `0, 0, 0` | `Y is N and Z is U` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | OWNER | `24381/48631` | `/1CAV-HVACHB` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | POS | `0, -1, 3.5` | `X 0mm, Y -1mm, Z 3.5mm` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | PSPE | `0/0` | `unset` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | REFNO | `24381/48945` | `24381_48945` |
| ZONE /-KX-CAV-9-HVAC-SUPP (=24381/48945) | TSPE | `0/0` | `unset` |
| ZONE /-RX-CCV-S02 (=24381/58336) | AREA | `unset` | `0` |
| ZONE /-RX-CCV-S02 (=24381/58336) | DESC | `安全壳连续通风系统` | `unset` |
| ZONE /-RX-CCV-S02 (=24381/58336) | ISPE | `0/0` | `unset` |
| ZONE /-RX-CCV-S02 (=24381/58336) | NUMB | `unset` | `0` |
| ZONE /-RX-CCV-S02 (=24381/58336) | ORI | `0, 0, 0` | `Y is N and Z is U` |
| ZONE /-RX-CCV-S02 (=24381/58336) | OWNER | `24381/57476` | `/1CCV-HVACHB` |
| ZONE /-RX-CCV-S02 (=24381/58336) | POS | `0, 0, 0` | `X 0mm, Y 0mm, Z 0mm` |
| ZONE /-RX-CCV-S02 (=24381/58336) | PSPE | `0/0` | `unset` |
| ZONE /-RX-CCV-S02 (=24381/58336) | REFNO | `24381/58336` | `24381_58336` |
| ZONE /-RX-CCV-S02 (=24381/58336) | TSPE | `0/0` | `unset` |
| ZONE /-RX-CCV-11-HVAC-SUPP (=24381/59496) | AREA | `unset` | `0` |
| ZONE /-RX-CCV-11-HVAC-SUPP (=24381/59496) | DESC | `安全壳连续通风系统` | `unset` |
| ZONE /-RX-CCV-11-HVAC-SUPP (=24381/59496) | ISPE | `0/0` | `unset` |
| ZONE /-RX-CCV-11-HVAC-SUPP (=24381/59496) | NUMB | `unset` | `0` |
| ZONE /-RX-CCV-11-HVAC-SUPP (=24381/59496) | ORI | `0, 0, 0` | `Y is N and Z is U` |
| ZONE /-RX-CCV-11-HVAC-SUPP (=24381/59496) | OWNER | `24381/57476` | `/1CCV-HVACHB` |
| ZONE /-RX-CCV-11-HVAC-SUPP (=24381/59496) | POS | `0, 0, 0` | `X 0mm, Y 0mm, Z 0mm` |
| ZONE /-RX-CCV-11-HVAC-SUPP (=24381/59496) | PSPE | `0/0` | `unset` |

### 只在服务供数有的字段（140 个字段名，按出现的元素数）

| 字段 | 元素数 |
|---|---:|
| `3D_FAMX` | 200 |
| `3D_FAZT` | 200 |
| `3D_GCBG` | 200 |
| `3D_GXZH` | 200 |
| `3D_JDRY` | 200 |
| `3D_KKZT` | 200 |
| `3D_MXGH` | 200 |
| `3D_PZJC` | 200 |
| `3D_PZRY` | 200 |
| `3D_SDRY` | 200 |
| `3D_SHRY` | 200 |
| `3D_SJBG` | 200 |
| `3D_SJJD` | 200 |
| `3D_SJRY` | 200 |
| `3D_SJZT` | 200 |
| `3D_THZT` | 200 |
| `3D_WCZT` | 200 |
| `MDSComment` | 200 |
| `MDSCp1` | 200 |
| `MDSCp2` | 200 |
| `MDSCp3` | 200 |
| `MDSVprmMto` | 200 |
| `PSIWEIGHT` | 200 |
| `TYPEX` | 200 |
| `⚠ diagnostics` | 200 |
| `G_STATUS` | 187 |
| `PLANREF1` | 152 |
| `PLANREF2` | 152 |
| `PLANREF3` | 152 |
| `CACHID` | 142 |
| `MDSOrigin` | 142 |
| `MDSSref` | 142 |
| `Room` | 142 |
| `G_ICS` | 89 |
| `BWHD` | 78 |
| `CATY` | 78 |
| `CONF` | 78 |
| `DWJZ` | 78 |
| `EPower` | 78 |
| `EQNA` | 78 |
| `ICSR-DoseRate-Acci` | 78 |
| `ICSR-DoseRate-NrOp` | 78 |
| `ICSR-DoseRate-NrSh` | 78 |
| `INST` | 78 |
| `JKGN` | 78 |
| `LTLX` | 78 |
| `MODE` | 78 |
| `OriPosition` | 78 |
| `PIDPOS` | 78 |
| `PIDREF` | 78 |
| …还有 90 个字段 | |

### 只在库供数有的字段（41 个字段名，按出现的元素数）

| 字段 | 元素数 |
|---|---:|
| `LOCK` | 191 |
| `ORRF` | 191 |
| `FSTAT` | 188 |
| `INPRTR` | 178 |
| `OUPRTR` | 178 |
| `SKEY` | 178 |
| `STMF` | 155 |
| `FUNC` | 154 |
| `INVF` | 142 |
| `USRCOG` | 142 |
| `USRWCO` | 142 |
| `USRWEI` | 142 |
| `USRWWE` | 142 |
| `UWMTXT` | 142 |
| `MDSYSF` | 123 |
| `DESC` | 113 |
| `INSC` | 112 |
| `PTSP` | 112 |
| `DSCO` | 98 |
| `STLR` | 87 |
| `DESP` | 78 |
| `SPRE` | 78 |
| `UKBOT` | 64 |
| `DUNIO` | 59 |
| `RLST` | 39 |
| `CARE` | 36 |
| `CDRG` | 36 |
| `CNUM` | 36 |
| `SPLP` | 36 |
| `DUTY` | 34 |
| `FAREA` | 26 |
| `FDRA` | 26 |
| `FPLINE` | 26 |
| `FRDR` | 26 |
| `FREV` | 26 |
| `LOOS` | 26 |
| `MODU` | 13 |
| `FACODE` | 11 |
| `UDTYPE` | 11 |
| `NAME` | 3 |
| `UMCDT` | 2 |

### 只在一边有的字段样例（前 50 条）

- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `⚠ diagnostics`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_FAMX`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_FAZT`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_GCBG`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_GXZH`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_JDRY`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_KKZT`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_MXGH`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_PZJC`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_PZRY`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_SDRY`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_SHRY`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_SJBG`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_SJJD`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_SJRY`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_SJZT`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_THZT`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `3D_WCZT`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABCLID`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABDECODE`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABEDATE`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABEFID`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABELIST`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABETIME`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABIDATE`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABITIME`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABMODNR`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABPRID`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABREVNO`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABSOURCE`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABSTID`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABTARGET`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABTRANO`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `FABTRRVNO`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `G_Export`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `MDSComment`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `MDSCp1`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `MDSCp2`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `MDSCp3`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `MDSVprmMto`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `PSIWEIGHT`
- 只在服务供数：SITE /1RB-CIVI (=24381/42520) · `TYPEX`
- 只在库供数：SITE /1RB-CIVI (=24381/42520) · `LOCK`
- 只在库供数：SITE /1RB-CIVI (=24381/42520) · `MODU`
- 只在库供数：SITE /1RB-CIVI (=24381/42520) · `ORRF`
- 只在库供数：SITE /1RB-CIVI (=24381/42520) · `RLST`
- 只在库供数：SITE /1RB-CIVI (=24381/42520) · `STMF`
- 只在库供数：SITE /1RB-CIVI (=24381/42520) · `UMCDT`
- 只在服务供数：SITE /1RX03-PIPEBJ (=24384/46) · `⚠ diagnostics`
- 只在服务供数：SITE /1RX03-PIPEBJ (=24384/46) · `3D_FAMX`

## 三维实例

抽样根 10 个：ZONE /1RB-WF05 (=24381/44279)、ZONE /1RX-RM (=24381/34110)、ZONE /-KX-CAV-E02 (=24381/57168)、ZONE /-RX-CCV-S11 (=24381/59076)、ZONE /-RX-CPV-2-HVAC-SUPP (=24381/76994)、ZONE /-SR-CSV-S01 (=24381/88959)、ZONE /1RND-LX-SUPP (=24381/101409)、ZONE /Copy-(2)-of-1RX4-YK-TUBE (=24381/105335)、ZONE /Copy-of-R23支架 (=24381/136389)、ZONE /Copy-of-检修通道模拟(含设备保温) (=24381/171257)

键 = `refno + geo_hash`，按集合比（同一元素下重复的键只算一次）；桶按库号分，库号由根的搜索命中解出（refno 高 32 位是 db ref，不是库号），解不出的桶按 db ref 列。

| 桶 | 服务 | 库 | 共有 | 只在服务 | 只在库 |
|---|---:|---:|---:|---:|---:|
| db7997 | 2014 | 2014 | 2014 | 0 | 0 |

