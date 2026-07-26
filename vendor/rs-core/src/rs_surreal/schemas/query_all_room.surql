select value (array::concat(REFNO<-pe_owner.in<-pe_owner.in<-pe_owner[where in.refno.NAME != NONE && in.noun == 'FRMW'].in.refno.NAME)) from (
select REFNO from SITE where NAME != NONE && string::contains(NAME,'ARCH'));
