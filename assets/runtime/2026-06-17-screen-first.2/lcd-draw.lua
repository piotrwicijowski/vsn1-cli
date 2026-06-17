local z=vsn1_cli_state
local t=os.clock()
local l="p"
if z.f.u>t then l="f" elseif z.s.u>t then l="s" end
if z.r==0 and z.w==l then return end
z.r=0 z.w=l
self:ldaf(0,0,319,239,c[1])self:ldrr(3,3,317,237,10,c[2])
if l=="f" then self:ldrrf(18,78,302,162,12,c[3])self:ldft(z.f.a,26,111,24,c[2])self:ldsw()return end
if l=="s" then self:ldrrf(18,78,302,162,12,c[2])self:ldft(z.s.m,26,111,24,c[1])self:ldsw()return end
local p=z.p local n,x,v=p.n,p.x,p.v local a=0
if x>n then a=(v-n)/(x-n)end
if a<0 then a=0 elseif a>1 then a=1 end
self:ldft(p.t,18,18,18,c[2])self:ldft("B"..p.k,274,18,18,c[2])self:ldft(p.b,18,188,18,c[2])self:ldrr(19,78,301,142,10,c[2])local r=20+math.floor(280*a)if r>20 then self:ldrrf(20,79,r,141,9,c[3])end self:ldft(tostring(v),134,101,24,c[2])self:ldsw()
