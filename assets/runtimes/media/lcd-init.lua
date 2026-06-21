glsb(255)z={r=1,w="",b="base",u={},o={"playback_status"},m={base=0,playback_status=2},l={base={a="",t="",l="",d=0,p=0},playback_status={s=""}}}vsn1_cli_state=z
c=c or{{0,0,0},{255,255,255},{64,160,255},{160,160,160},{208,208,208}}
function set_field(l,k,v)local q=z.l[l]if q then q[k]=v z.r=1 end end
function activate_layer(l)local t=z.m[l]if t==nil then return end if t>0 then z.u[l]=os.clock()+t else z.b=l end z.r=1 end
function T(s)s=math.max(0,math.floor((tonumber(s)or 0)+.5))local h=math.floor(s/3600)local m=math.floor(s/60)%60 local r=s%60 if h>0 then return("%d:%02d:%02d"):format(h,m,r)end return("%d:%02d"):format(m,r)end
