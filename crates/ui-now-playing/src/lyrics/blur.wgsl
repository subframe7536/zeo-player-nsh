// 全屏歌词的渐进模糊与边缘淡出（`blur.rs` 的 `shader_effect` 用）。
//
// 按视口位置算，不跟高亮行走：锚点附近一条固定的清晰带（跟随播放时
// 高亮行正停在那里），离开它在 `ramp` 内线性涨到 `max_radius`；手动滚开
// 时滚进带里的行变清晰、高亮行滚出去变糊。
//
// 模糊：黄金角螺旋在圆盘内取 TAPS 个点、按高斯加权（`sigma` 同 CSS
// `blur()` 的 σ，取样圆盘半径 SPREAD·σ）；每个像素把螺旋转一个伪随机
// 角度，采样不足的规律纹样变成细噪点。在预乘 alpha 下累加：文字周围
// 是透明的，直接平均直通色会混进透明像素的黑 rgb，边缘发灰。

struct Uniforms {
    // 清晰带上下沿（视口内的 y，点）。
    clear_top: f32,
    clear_bottom: f32,
    // 离开清晰带多远（点）模糊到最大。
    ramp: f32,
    // 最大模糊（σ，点）。
    max_radius: f32,
    // 0..1：悬停歌词区时淡到 0（全部清晰）；无时间轴的歌词恒为 0。
    amount: f32,
    // 上下边缘淡出区（视口高度的比例）。
    fade_top: f32,
    fade_bottom: f32,
}

const TAPS: i32 = 64;
const GOLDEN: f32 = 2.39996323;
const SPREAD: f32 = 2.5;

fn premul(c: vec4f) -> vec4f {
    return vec4f(c.rgb * c.a, c.a);
}

fn sigma_at(y: f32) -> f32 {
    let d = max(max(uniforms.clear_top - y, y - uniforms.clear_bottom), 0.0);
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
    let pixel = uv * nsh.size * nsh.scale;
    // Interleaved gradient noise：每像素一个旋转角。
    let jitter = fract(52.9829189 * fract(dot(pixel, vec2f(0.06711056, 0.00583715)))) * 6.2831853;
    let reach = sigma * SPREAD;
    var acc = vec4f(0.0);
    var total = 0.0;
    for (var i = 0; i < TAPS; i++) {
        let t = (f32(i) + 0.5) / f32(TAPS);
        let a = f32(i) * GOLDEN + jitter;
        let offset = vec2f(cos(a), sin(a)) * sqrt(t) * reach;
        // 均匀圆盘取样，t = (半径 / reach)²：高斯权重 exp(-(√t·SPREAD)² / 2)。
        let w = exp(-0.5 * SPREAD * SPREAD * t);
        acc += premul(sample_content(uv + offset / nsh.size)) * w;
        total += w;
    }
    let p = acc / total;
    if (p.a <= 0.0) {
        return vec4f(0.0);
    }
    return vec4f(p.rgb / p.a, p.a);
}

fn shade(uv: vec2f) -> vec4f {
    let y = uv.y * nsh.size.y;
    let sigma = sigma_at(y);
    var c = sample_content(uv);
    if (sigma >= 0.3) {
        c = blurred(uv, sigma);
    }
    return vec4f(c.rgb, c.a * edge_fade(y));
}
