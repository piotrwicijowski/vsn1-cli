glsb(255)local function I()return{"---","---","---","---","---","---","---","---"}end
z={r=1,w="",b="persistent",u={},o={"slow","fast"},m={persistent=0,slow=5,fast=1},l={persistent={t="",b="",v=0,n=0,x=127,d=-1,s=0,i=I(),l=false,h=false,k=0},slow={m=""},fast={a=""}}}vsn1_cli_state=z
c=c or{{0,0,0},{255,255,255},{64,160,255}}
function set_field(l,k,v)local q=z.l[l]if q then q[k]=v z.r=1 end end
function activate_layer(l)local t=z.m[l]if t==nil then return end if t>0 then z.u[l]=os.clock()+t else z.b=l end z.r=1 end
