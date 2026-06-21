local z=vsn1_cli_state
local t=os.clock()local l,p,e=z.b,z.l,z.u
for i=1,#z.o do local n=z.o[i]if(e[n]or 0)>t then l=n end end
if z.r==0 and z.w==l then return end
z.r=0 z.w=l
self:ldaf(0,0,319,239,{0,0,0})self:ldrr(3,3,317,237,10,{255,255,255})
local q=p[l]
if l=="fast" then self:ldrrf(18,78,302,162,12,{64,160,255})self:ldft(q.a,26,111,24,{255,255,255})self:ldsw()return elseif l=="slow" then self:ldft(q.m,26,111,24,{255,255,255})self:ldsw()return end
local n,x,v=q.n,q.x,q.v local a=0
if x>n then a=(v-n)/(x-n)end
if a<0 then a=0 elseif a>1 then a=1 end
self:ldft(q.t,18,18,18,{255,255,255})self:ldft("B"..q.k,274,18,18,{255,255,255})self:ldft(q.b,18,188,18,{255,255,255})self:ldrr(19,78,301,142,10,{255,255,255})local r=20+math.floor(280*a)if r>20 then self:ldrrf(20,79,r,141,9,{64,160,255})end self:ldft(tostring(v),134,101,24,{255,255,255})self:ldsw()
