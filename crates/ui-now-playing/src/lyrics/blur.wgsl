// 全屏歌词的渐进模糊与边缘淡出（`blur.rs` 的 `shader_effect` 用）。
//
// 按视口位置算，不跟高亮行走：锚点附近一条固定的清晰带（跟随播放时
// 高亮行正停在那里），离开它在 `ramp` 内线性涨到 `max_radius`；手动滚开
// 时滚进带里的行变清晰、高亮行滚出去变糊。
//
// 模糊：黄金角螺旋在圆盘内取样、按高斯加权（`sigma` 同 CSS `blur()` 的
// σ，取样圆盘半径 SPREAD·σ），按 `sigma` 档位取 8 / 16 个点，并用增量
// 旋转与权重递推省去循环内三角 / 指数计算；每个像素把螺旋转一个伪随机
// 角度，采样不足的规律纹样变成细噪点。在预乘 alpha 下累加：文字周围
// 是透明的，直接平均直通色会混进透明像素的黑 rgb，边缘发灰。

struct Uniforms {
    // 锚点位置（视口高度的比例）。
    anchor: f32,
    // 清晰带在锚点之上 / 之下的范围（点）。
    clear_above: f32,
    clear_below: f32,
    // 离开清晰带多远（点）模糊到最大。
    ramp: f32,
    // 最大模糊（σ，点）。
    max_radius: f32,
    // 0..1：悬停歌词区时由平台动画淡到 0（全部清晰）；无时间轴的歌词恒为 0。
    amount: f32,
    // 上下边缘淡出区（视口高度的比例）。
    fade_top: f32,
    fade_bottom: f32,
}

const SPREAD: f32 = 2.0;
// 黄金角 2.39996323 rad 的 cos / sin，每步旋转采样方向。
const COS_GOLDEN: f32 = -0.73736888;
const SIN_GOLDEN: f32 = 0.67549029;

fn premul(c: vec4f) -> vec4f {
    return vec4f(c.rgb * c.a, c.a);
}

fn sigma_at(y: f32) -> f32 {
    if (uniforms.amount <= 0.0) {
        return 0.0;
    }
    let anchor_y = nsh.size.y * uniforms.anchor;
    let clear_top = anchor_y - uniforms.clear_above;
    let clear_bottom = anchor_y + uniforms.clear_below;
    let d = max(max(clear_top - y, y - clear_bottom), 0.0);
    return uniforms.max_radius * uniforms.amount * clamp(d / max(uniforms.ramp, 1.0), 0.0, 1.0);
}

// 行在上下边缘淡出：进入顶部 / 底部淡出区后线性降到 0。
fn edge_fade(y: f32) -> f32 {
    let h = nsh.size.y;
    let top = y / max(h * uniforms.fade_top, 1.0);
    let bottom = (h - y) / max(h * uniforms.fade_bottom, 1.0);
    return clamp(min(top, bottom), 0.0, 1.0);
}

fn blurred(uv: vec2f, sigma: f32) -> vec4f {
    let taps = select(16, 8, sigma < 1.8);
    let inv_n = 1.0 / f32(taps);
    let pixel = uv * nsh.size * nsh.scale;
    // Interleaved gradient noise：每像素一个旋转角。
    let jitter = fract(52.9829189 * fract(dot(pixel, vec2f(0.06711056, 0.00583715)))) * 6.2831853;
    var dir = vec2f(cos(jitter), sin(jitter));
    let reach = sigma * SPREAD;
    let inv_size = 1.0 / max(nsh.size, vec2f(1.0));
    // 均匀圆盘取样，t = (i + 0.5) / taps：高斯权重 exp(-0.5·SPREAD²·t) 逐项乘常数。
    let k = 0.5 * SPREAD * SPREAD;
    let w_step = exp(-k * inv_n);
    var w = exp(-k * 0.5 * inv_n);
    var acc = vec4f(0.0);
    var total = 0.0;
    for (var i = 0; i < taps; i++) {
        let t = (f32(i) + 0.5) * inv_n;
        let offset = dir * (sqrt(t) * reach);
        acc += premul(sample_content(uv + offset * inv_size)) * w;
        total += w;
        w *= w_step;
        dir = vec2f(
            dir.x * COS_GOLDEN - dir.y * SIN_GOLDEN,
            dir.x * SIN_GOLDEN + dir.y * COS_GOLDEN,
        );
    }
    let p = acc / total;
    if (p.a <= 0.0) {
        return vec4f(0.0);
    }
    return vec4f(p.rgb / p.a, p.a);
}

fn shade(uv: vec2f) -> vec4f {
    let y = uv.y * nsh.size.y;
    let fade = edge_fade(y);
    if (fade <= 0.0) {
        return vec4f(0.0);
    }
    let sigma = sigma_at(y);
    var c: vec4f;
    if (sigma >= 0.3) {
        c = blurred(uv, sigma);
    } else {
        c = sample_content(uv);
    }
    return vec4f(c.rgb, c.a * fade);
}
