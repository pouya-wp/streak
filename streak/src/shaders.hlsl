// Streak — overlay shaders. Everything is premultiplied alpha.

cbuffer Frame : register(b0)
{
    float2 invScreen;   // 1 / render target size
    float  time;
    float  pad0;
    uint   baseIndex;   // first instance / vertex for this draw
    uint3  pad1;
};

// ---------------------------------------------------------------------------
// Instanced quads: cursor sprites, glow, particles, click ripples
// ---------------------------------------------------------------------------

struct Inst
{
    float2 center;
    float2 halfSize;
    float4 color;    // straight rgb, a = opacity / weight
    float4 p;        // x = kind, y = rotation, z = param1, w = additive (0..1)
    float4 uv;       // uv rect (cursor sprites)
};

StructuredBuffer<Inst> insts   : register(t0);
Texture2D              tex     : register(t1);
SamplerState           samp    : register(s0);

struct QOut
{
    float4 pos   : SV_Position;
    float2 uv    : TEXCOORD0;
    float2 local : TEXCOORD1;
    nointerpolation float4 color : COLOR0;
    nointerpolation float4 p     : TEXCOORD2;
};

QOut VSQuad(uint vid : SV_VertexID, uint iid : SV_InstanceID)
{
    Inst it = insts[baseIndex + iid];
    float2 c = float2(vid & 1, vid >> 1);
    float2 l = c * 2.0 - 1.0;
    float s, co;
    sincos(it.p.y, s, co);
    float2 o = l * it.halfSize;
    o = float2(o.x * co - o.y * s, o.x * s + o.y * co);
    float2 px = it.center + o;

    QOut r;
    r.pos = float4(px * invScreen * float2(2, -2) + float2(-1, 1), 0, 1);
    r.uv = lerp(it.uv.xy, it.uv.zw, c);
    r.local = l;
    r.color = it.color;
    r.p = it.p;
    return r;
}

float aaStep(float edge, float x)
{
    float w = max(fwidth(x), 1e-4);
    return saturate((edge - x) / w + 0.5);
}

float4 PSQuad(QOut i) : SV_Target
{
    int kind = (int)(i.p.x + 0.5);
    float2 l = i.local;
    float d = length(l);
    float a = 0;

    if (kind == 0)
    {
        // Cursor sprite. color.rgb = tint, p.z = tint amount, color.a = weight.
        float4 t = tex.Sample(samp, i.uv);
        float3 rgb = lerp(t.rgb, t.a * i.color.rgb, i.p.z);
        float4 o = float4(rgb, t.a) * i.color.a;
        o.a *= (1.0 - i.p.w);
        return o;
    }
    else if (kind == 1)
    {
        // Soft radial glow. p.z = falloff exponent.
        a = pow(saturate(1.0 - d), max(i.p.z, 0.05));
    }
    else if (kind == 2)
    {
        // Ring. p.z = thickness as fraction of radius.
        float t = max(i.p.z, 0.01);
        float dist = abs(d - (1.0 - t * 0.5));
        a = aaStep(t * 0.5, dist);
    }
    else if (kind == 3)
    {
        // 4-point sparkle star.
        float2 q = abs(l);
        float rays = max(exp(-q.x * 9.0) * saturate(1.0 - q.y), exp(-q.y * 9.0) * saturate(1.0 - q.x));
        float core = pow(saturate(1.0 - d * 1.6), 2.0);
        a = saturate(rays + core);
    }
    else if (kind == 4)
    {
        // Solid anti-aliased disc.
        a = aaStep(1.0, d);
    }
    else if (kind == 5)
    {
        // Square (confetti).
        float2 q = abs(l);
        a = aaStep(1.0, max(q.x, q.y));
    }
    else if (kind == 6)
    {
        // Heart (SDF-ish). y flipped so the heart points down.
        float2 q = float2(abs(l.x), -l.y * 1.05 + 0.25);
        float h = q.x * q.x + pow(q.y - pow(q.x, 0.66) * 0.65, 2.0);
        a = aaStep(0.55, h);
    }
    else if (kind == 7)
    {
        // Spark: thin elongated streak (rotation aligns it with velocity).
        float2 q = abs(l);
        a = saturate(1.0 - q.x) * exp(-q.y * q.y * 18.0);
        a = a * a;
    }
    else if (kind == 8)
    {
        // Soft drop shadow for the cursor: sample the sprite alpha with a wide kernel.
        float acc = 0;
        [unroll] for (int k = 0; k < 12; k++)
        {
            float ang = k * 0.5235988;
            float2 off = float2(cos(ang), sin(ang)) * i.p.z;
            acc += tex.Sample(samp, i.uv + off).a;
            acc += tex.Sample(samp, i.uv + off * 0.5).a;
        }
        a = acc / 24.0;
    }

    float4 o = float4(i.color.rgb, 1.0) * (a * i.color.a);
    o.a *= (1.0 - i.p.w);
    return o;
}

// ---------------------------------------------------------------------------
// Ribbon trail: plain triangle list built on the CPU
// ---------------------------------------------------------------------------

struct RVert
{
    float2 pos;
    float  v;        // -1..1 across the ribbon
    float  soft;     // softness
    float4 color;    // straight rgb, a = opacity
    float  additive;
    float3 pad;
};

StructuredBuffer<RVert> rverts : register(t2);

struct ROut
{
    float4 pos   : SV_Position;
    float  v     : TEXCOORD0;
    float  soft  : TEXCOORD1;
    float4 color : COLOR0;
    float  add   : TEXCOORD2;
};

ROut VSRibbon(uint vid : SV_VertexID)
{
    RVert rv = rverts[baseIndex + vid];
    ROut r;
    r.pos = float4(rv.pos * invScreen * float2(2, -2) + float2(-1, 1), 0, 1);
    r.v = rv.v;
    r.soft = rv.soft;
    r.color = rv.color;
    r.add = rv.additive;
    return r;
}

float4 PSRibbon(ROut i) : SV_Target
{
    float x = abs(i.v);
    float edge = 1.0 - smoothstep(1.0 - max(i.soft, 0.02), 1.0, x);
    float core = pow(saturate(1.0 - x), 3.0) * 0.6 * i.add;
    float a = saturate(edge + core) * i.color.a;
    float4 o = float4(i.color.rgb, 1.0) * a;
    o.a *= (1.0 - i.add);
    return o;
}
