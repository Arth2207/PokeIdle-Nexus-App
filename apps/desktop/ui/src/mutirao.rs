//! "Mutirão Solar": a faixa xilogravada no topo de cada tela. A mesma cena é desenhada duas
//! vezes: em tinta preta sobre papel cru (sempre visível) e em cores (só onde o sol ilumina).
//! O sol segue o cursor (ou o toque): raios girando, tamanho pela velocidade, sombras saindo
//! dele. Sem o cursor na faixa, ele descansa no canto. Os dados da tela nunca ficam embaixo.
//! Tudo SVG desenhado à mão aqui (sem imagens, ícones ou emoji).

use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

/// Largura e altura da cena (unidades do viewBox).
const W: f64 = 1200.0;
const H: f64 = 240.0;
/// Linha do chão.
const CHAO: f64 = 214.0;

/// Sombra de uma peça: um chão escuro que tomba para longe do sol (`--sx`, de −1 a 1).
fn sombra(x: f64, largura: f64) -> String {
    format!(
        r#"<ellipse class="sombra" cx="{cx}" cy="{CHAO}" rx="{rx}" ry="7" style="transform-origin:{cx}px {CHAO}px"/>"#,
        cx = x + largura / 2.0,
        rx = largura * 0.62
    )
}

/// Casa de taipa com telhado verde (grama e flores no telhado).
fn casa(x: f64, parede: &str) -> String {
    let y = CHAO - 64.0;
    format!(
        r#"<g class="peca casa">
<rect x="{x}" y="{y}" width="96" height="64" class="{parede}" />
<rect x="{xs}" y="{y}" width="22" height="64" fill="url(#mt-hach)" class="t-hach" />
<path d="M{a} {y} L{b} {yt} L{c} {yt} L{d} {y} Z" class="c-verde" />
<path d="M{b2} {yt} q4 -9 8 0 q4 -11 8 0 q4 -8 8 0 q5 -12 9 0 q4 -9 8 0 q4 -10 8 0 q4 -8 8 0" class="c-verde grama" />
<circle class="flor c-hib" cx="{f1}" cy="{yf}" r="4" /><circle class="flor c-sol" cx="{f2}" cy="{yf2}" r="3.5" />
<rect x="{xp}" y="{yp}" width="20" height="34" class="c-urucum" />
<rect x="{xj}" y="{yj}" width="18" height="16" class="c-azul jan" />
<path d="M{xj} {yjm} h18 M{xjm} {yj} v16" class="linha" />
</g>"#,
        xs = x + 74.0,
        a = x - 8.0,
        b = x + 10.0,
        c = x + 86.0,
        d = x + 104.0,
        yt = y - 22.0,
        b2 = x + 14.0,
        f1 = x + 30.0,
        f2 = x + 62.0,
        yf = y - 26.0,
        yf2 = y - 27.0,
        xp = x + 14.0,
        yp = CHAO - 34.0,
        xj = x + 48.0,
        yj = y + 16.0,
        yjm = y + 24.0,
        xjm = x + 57.0,
    )
}

/// Horta vertical: armação com quatro prateleiras de plantas.
fn horta(x: f64) -> String {
    let topo = CHAO - 120.0;
    let mut s = format!(
        r#"<g class="peca horta"><rect x="{x}" y="{topo}" width="46" height="120" class="c-madeira" />"#
    );
    for i in 0..4 {
        let yy = topo + 14.0 + f64::from(i) * 28.0;
        s.push_str(&format!(
            r#"<rect x="{x}" y="{yy}" width="46" height="8" class="c-terra" />
<path class="folhagem c-verde" d="M{a} {yy} q4 -14 8 0 q4 -16 8 0 q4 -12 8 0 q4 -15 8 0 q4 -13 8 0" />
<circle class="flor {cor}" cx="{fx}" cy="{fy}" r="3.4" />"#,
            a = x + 3.0,
            cor = ["c-hib", "c-sol", "c-urucum", "c-hib"][i as usize],
            fx = x + 12.0 + f64::from(i % 2) * 20.0,
            fy = yy - 8.0,
        ));
    }
    s.push_str("</g>");
    s
}

/// Painel solar em forma de folha num poste.
fn painel(x: f64) -> String {
    let y = CHAO - 112.0;
    format!(
        r#"<g class="peca painel-folha">
<path d="M{px} {CHAO} V{yp}" class="linha grossa" />
<path class="painel c-azul" d="M{px} {yp} C{a} {b} {c} {d} {px} {y} C{e} {d} {f} {b} {px} {yp} Z" />
<path d="M{px} {yp} V{y} M{l1} {ya} L{px} {yb} L{r1} {ya} M{l2} {yc} L{px} {yd} L{r2} {yc}" class="linha fina" />
</g>"#,
        px = x + 30.0,
        yp = y + 64.0,
        a = x - 6.0,
        b = y + 44.0,
        c = x + 2.0,
        d = y + 8.0,
        e = x + 58.0,
        f = x + 66.0,
        l1 = x + 14.0,
        r1 = x + 46.0,
        ya = y + 28.0,
        yb = y + 40.0,
        l2 = x + 10.0,
        r2 = x + 50.0,
        yc = y + 46.0,
        yd = y + 56.0,
    )
}

/// Cisterna com água que ondula.
fn cisterna(x: f64) -> String {
    let y = CHAO - 58.0;
    format!(
        r#"<g class="peca cisterna">
<rect x="{x}" y="{y}" width="60" height="58" rx="6" class="c-kraft" />
<rect x="{x}" y="{ya}" width="60" height="34" rx="4" class="c-agua agua" />
<path class="onda" d="M{x} {ya} q7 -5 15 0 t15 0 t15 0 t15 0" />
<ellipse cx="{cx}" cy="{y}" rx="30" ry="7" class="c-kraft" />
<path d="M{cx} {yc} v-18 h40" class="linha" />
</g>"#,
        ya = y + 24.0,
        cx = x + 30.0,
        yc = y - 7.0,
    )
}

/// Pessoa: cabeça, corpo de chita, braços (um acena), pernas.
fn pessoa(x: f64, roupa: &str, acena: bool) -> String {
    let y = CHAO - 52.0;
    let braco = if acena {
        format!(
            r#"<path class="braco acena" d="M{a} {b} l14 -16" style="transform-origin:{a}px {b}px" />"#,
            a = x + 14.0,
            b = y + 20.0
        )
    } else {
        format!(
            r#"<path class="braco" d="M{a} {b} l12 12" />"#,
            a = x + 14.0,
            b = y + 20.0
        )
    };
    format!(
        r#"<g class="peca pessoa">
<circle cx="{cx}" cy="{y}" r="8" class="c-pele" />
<path d="M{a} {y2} h12 l5 28 h-22 Z" class="{roupa}" />
<path d="M{l} {y3} l-10 12" class="braco" />{braco}
<path d="M{p1} {y4} v16 M{p2} {y4} v16" class="linha grossa" />
</g>"#,
        cx = x + 8.0,
        a = x + 2.0,
        y2 = y + 10.0,
        l = x + 2.0,
        y3 = y + 20.0,
        p1 = x + 4.0,
        p2 = x + 13.0,
        y4 = y + 36.0,
    )
}

/// Bicicleta-gerador: rodas girando, ciclista, fio até uma lâmpada.
fn bicicleta(x: f64) -> String {
    let y = CHAO - 18.0;
    let roda = |cx: f64| {
        format!(
            r#"<g class="roda" style="transform-origin:{cx}px {y}px"><circle cx="{cx}" cy="{y}" r="16" class="c-kraft" /><path d="M{a} {y} h32 M{cx} {b} v32 M{c} {d} l22 22 M{c} {e} l22 -22" class="linha fina" /></g>"#,
            a = cx - 16.0,
            b = y - 16.0,
            c = cx - 11.0,
            d = y - 11.0,
            e = y + 11.0,
        )
    };
    format!(
        r#"<g class="peca bicicleta">{r1}{r2}
<path d="M{x1} {y} L{x2} {yq} L{x3} {y} M{x2} {yq} h20 L{x3} {y}" class="linha grossa" />
<circle cx="{hx}" cy="{hy}" r="7" class="c-pele" />
<path d="M{bx} {by} l8 22 l-6 14" class="c-sol linha grossa" />
<path d="M{x3} {y} q30 -10 40 -50" class="linha fina fio" />
<circle class="lampada c-sol" cx="{lx}" cy="{ly}" r="7" />
</g>"#,
        r1 = roda(x + 16.0),
        r2 = roda(x + 66.0),
        x1 = x + 16.0,
        x2 = x + 36.0,
        x3 = x + 66.0,
        yq = y - 22.0,
        hx = x + 44.0,
        hy = y - 50.0,
        bx = x + 42.0,
        by = y - 43.0,
        lx = x + 106.0,
        ly = y - 54.0,
    )
}

/// Abelha-drone: corpo listrado e quatro hélices.
fn abelha(x: f64, y: f64, i: u32) -> String {
    format!(
        r#"<g class="abelha a{i}" style="transform-origin:{x}px {y}px">
<ellipse cx="{x}" cy="{y}" rx="11" ry="7" class="c-sol" />
<path d="M{a} {b} v14 M{c} {b} v14" class="linha" />
<path class="helice" d="M{h1} {hy} h-9 M{h2} {hy} h9" />
</g>"#,
        a = x - 3.0,
        c = x + 3.0,
        b = y - 7.0,
        h1 = x - 8.0,
        h2 = x + 8.0,
        hy = y - 10.0,
    )
}

/// Bananeira: tronco e folhas largas.
fn bananeira(x: f64) -> String {
    let y = CHAO - 100.0;
    format!(
        r#"<g class="peca bananeira">
<path d="M{tx} {CHAO} C{tx} {a} {t2} {b} {t2} {y}" class="c-madeira linha grossa" />
<path class="folha c-verde" d="M{t2} {y} c-40 -6 -58 18 -66 34 c26 -4 48 -10 66 -34 z" style="transform-origin:{t2}px {y}px" />
<path class="folha c-verde" d="M{t2} {y} c36 -14 60 6 70 22 c-28 2 -50 -4 -70 -22 z" style="transform-origin:{t2}px {y}px" />
<path class="folha c-verde" d="M{t2} {y} c-8 -36 14 -54 30 -60 c0 24 -8 46 -30 60 z" style="transform-origin:{t2}px {y}px" />
<circle class="flor c-urucum" cx="{fx}" cy="{fy}" r="5" />
</g>"#,
        tx = x + 20.0,
        t2 = x + 26.0,
        a = CHAO - 40.0,
        b = CHAO - 70.0,
        fx = x + 34.0,
        fy = y + 22.0,
    )
}

/// Moinho de vento (turbina) com pás girando.
fn moinho(x: f64) -> String {
    let y = CHAO - 130.0;
    format!(
        r#"<g class="peca moinho">
<path d="M{a} {CHAO} L{cx} {y} L{b} {CHAO}" class="c-kraft linha" />
<g class="pas" style="transform-origin:{cx}px {y}px">
<path d="M{cx} {y} l-4 -46 l8 0 z M{cx} {y} l40 22 l-4 7 z M{cx} {y} l-40 22 l4 7 z" class="c-kraft linha" />
</g><circle cx="{cx}" cy="{y}" r="5" class="c-urucum" />
</g>"#,
        cx = x + 20.0,
        a = x + 12.0,
        b = x + 28.0,
    )
}

/// Barraca de feira com toldo listrado e frutas.
fn barraca(x: f64, listra: &str) -> String {
    let y = CHAO - 78.0;
    let mut toldo = String::new();
    for i in 0..6 {
        let xi = x + f64::from(i) * 18.0;
        let cor = if i % 2 == 0 { listra } else { "c-kraft" };
        toldo.push_str(&format!(
            r#"<path d="M{xi} {y} h18 v18 q-9 8 -18 0 Z" class="{cor}" />"#
        ));
    }
    format!(
        r#"<g class="peca barraca">
<path d="M{a} {y} V{CHAO} M{b} {y} V{CHAO}" class="linha grossa" />
{toldo}
<rect x="{x}" y="{ym}" width="108" height="10" class="c-madeira" />
<circle class="fruta c-urucum" cx="{f1}" cy="{yf}" r="7" /><circle class="fruta c-sol" cx="{f2}" cy="{yf}" r="7" />
<circle class="fruta c-verde" cx="{f3}" cy="{yf}" r="7" /><circle class="fruta c-hib" cx="{f4}" cy="{yf}" r="6" />
</g>"#,
        a = x + 4.0,
        b = x + 104.0,
        ym = CHAO - 34.0,
        yf = CHAO - 41.0,
        f1 = x + 22.0,
        f2 = x + 42.0,
        f3 = x + 64.0,
        f4 = x + 86.0,
    )
}

/// Fogueira de São João.
fn fogueira(x: f64) -> String {
    format!(
        r#"<g class="peca fogueira">
<path d="M{a} {CHAO} l40 -14 M{b} {CHAO} l-40 -14" class="c-madeira linha grossa" />
<path class="chama c-urucum" d="M{c} {y1} c-14 -16 2 -26 0 -44 c14 12 22 26 10 44 z" style="transform-origin:{c}px {CHAO}px" />
<path class="chama c-sol" d="M{c} {y1} c-6 -10 2 -16 2 -26 c8 8 10 18 4 26 z" style="transform-origin:{c}px {CHAO}px" />
</g>"#,
        a = x,
        b = x + 44.0,
        c = x + 20.0,
        y1 = CHAO - 8.0,
    )
}

/// Varal de bandeirinhas de festa junina entre dois pontos.
fn bandeirinhas(x1: f64, x2: f64, y: f64) -> String {
    let n = ((x2 - x1) / 26.0) as i32;
    let cores = ["c-urucum", "c-sol", "c-verde", "c-azul", "c-hib"];
    let mut s = format!(
        r#"<path d="M{x1} {y} Q{m} {yq} {x2} {y}" class="linha fina" />"#,
        m = (x1 + x2) / 2.0,
        yq = y + 26.0
    );
    for i in 0..n {
        let t = (f64::from(i) + 0.5) / f64::from(n);
        let x = x1 + (x2 - x1) * t;
        // Ponto da curva do varal (Bézier quadrática com controle 26 abaixo do meio).
        let yy = y + 52.0 * t * (1.0 - t) - 1.0;
        s.push_str(&format!(
            r#"<path class="bandeira {cor}" d="M{a} {yy} h16 l-8 16 z" style="transform-origin:{x}px {yy}px;animation-delay:-{d}s" />"#,
            cor = cores[(i as usize) % cores.len()],
            a = x - 8.0,
            d = f64::from(i) * 0.37,
        ));
    }
    s
}

/// Morros ao fundo e o chão com hachuras de goiva.
fn fundo() -> String {
    format!(
        r#"<path d="M0 {m} C180 {a} 300 {b} 470 {c} S780 {d} 960 {e} S1140 {f} 1200 {g} V{CHAO} H0 Z" class="c-morro" />
<path d="M0 {CHAO} H{W}" class="linha grossa" />
<rect x="0" y="{CHAO}" width="{W}" height="{hc}" fill="url(#mt-goiva)" class="t-hach" />"#,
        m = CHAO - 60.0,
        a = CHAO - 110.0,
        b = CHAO - 40.0,
        c = CHAO - 76.0,
        d = CHAO - 120.0,
        e = CHAO - 70.0,
        f = CHAO - 30.0,
        g = CHAO - 64.0,
        hc = H - CHAO,
    )
}

/// A cena de cada tela: (desenho, sombras).
fn cena(nome: &str) -> (String, String) {
    let mut d = fundo();
    let mut sombras = String::new();
    // Bandeirinhas e abelhas vão por cima das peças.
    let mut extra = String::new();
    let mut por = |s: String, x: f64, l: f64| {
        sombras.push_str(&sombra(x, l));
        d.push_str(&s);
    };
    match nome {
        "Caçada" => {
            por(bananeira(40.0), 40.0, 60.0);
            por(bananeira(180.0), 180.0, 60.0);
            por(pessoa(330.0, "c-urucum", true), 330.0, 22.0);
            por(pessoa(370.0, "c-azul", false), 370.0, 22.0);
            por(horta(470.0), 470.0, 46.0);
            por(bananeira(600.0), 600.0, 60.0);
            por(cisterna(760.0), 760.0, 60.0);
            por(painel(880.0), 880.0, 60.0);
            por(bananeira(1000.0), 1000.0, 60.0);
            extra.push_str(&abelha(300.0, 70.0, 1));
            extra.push_str(&abelha(720.0, 90.0, 2));
        }
        "Combate" => {
            extra.push_str(&bandeirinhas(20.0, 1180.0, 26.0));
            por(pessoa(160.0, "c-urucum", true), 160.0, 22.0);
            por(pessoa(200.0, "c-hib", false), 200.0, 22.0);
            por(pessoa(240.0, "c-azul", true), 240.0, 22.0);
            por(fogueira(560.0), 560.0, 44.0);
            por(pessoa(860.0, "c-verde", true), 860.0, 22.0);
            por(pessoa(900.0, "c-sol", false), 900.0, 22.0);
            por(pessoa(940.0, "c-urucum", true), 940.0, 22.0);
            por(moinho(1080.0), 1080.0, 40.0);
            por(casa(20.0, "c-taipa"), 20.0, 96.0);
        }
        "Mercado" | "Finanças" => {
            extra.push_str(&bandeirinhas(40.0, 1160.0, 30.0));
            por(barraca(60.0, "c-urucum"), 60.0, 108.0);
            por(pessoa(190.0, "c-hib", true), 190.0, 22.0);
            por(barraca(260.0, "c-azul"), 260.0, 108.0);
            por(pessoa(390.0, "c-verde", false), 390.0, 22.0);
            por(barraca(460.0, "c-verde"), 460.0, 108.0);
            por(cisterna(620.0), 620.0, 60.0);
            por(pessoa(710.0, "c-sol", true), 710.0, 22.0);
            por(barraca(770.0, "c-hib"), 770.0, 108.0);
            por(painel(920.0), 920.0, 60.0);
            por(casa(1020.0, "c-taipa"), 1020.0, 96.0);
        }
        "Bestiário" => {
            por(horta(60.0), 60.0, 46.0);
            por(horta(130.0), 130.0, 46.0);
            por(bananeira(220.0), 220.0, 60.0);
            por(pessoa(340.0, "c-azul", false), 340.0, 22.0);
            por(pessoa(376.0, "c-hib", true), 376.0, 22.0);
            por(horta(470.0), 470.0, 46.0);
            por(bananeira(560.0), 560.0, 60.0);
            por(casa(700.0, "c-taipa"), 700.0, 96.0);
            por(horta(860.0), 860.0, 46.0);
            por(bananeira(960.0), 960.0, 60.0);
            for (i, (x, y)) in [(280.0, 60.0), (520.0, 80.0), (820.0, 56.0), (1100.0, 90.0)]
                .iter()
                .enumerate()
            {
                extra.push_str(&abelha(*x, *y, i as u32 + 1));
            }
        }
        "Automação" | "Configurações" | "Jogo" => {
            por(moinho(60.0), 60.0, 40.0);
            por(moinho(150.0), 150.0, 40.0);
            por(painel(240.0), 240.0, 60.0);
            por(painel(320.0), 320.0, 60.0);
            por(bicicleta(430.0), 430.0, 90.0);
            por(casa(600.0, "c-taipa"), 600.0, 96.0);
            por(cisterna(740.0), 740.0, 60.0);
            por(pessoa(840.0, "c-urucum", true), 840.0, 22.0);
            por(moinho(940.0), 940.0, 40.0);
            por(painel(1040.0), 1040.0, 60.0);
            extra.push_str(&abelha(560.0, 70.0, 1));
        }
        // Visão geral: o mutirão inteiro.
        _ => {
            extra.push_str(&bandeirinhas(380.0, 820.0, 34.0));
            por(moinho(20.0), 20.0, 40.0);
            por(casa(90.0, "c-taipa"), 90.0, 96.0);
            por(horta(230.0), 230.0, 46.0);
            por(painel(290.0), 290.0, 60.0);
            por(pessoa(380.0, "c-urucum", true), 380.0, 22.0);
            por(pessoa(414.0, "c-azul", false), 414.0, 22.0);
            por(pessoa(448.0, "c-hib", true), 448.0, 22.0);
            por(bicicleta(500.0), 500.0, 90.0);
            por(cisterna(660.0), 660.0, 60.0);
            por(casa(760.0, "c-taipa-2"), 760.0, 96.0);
            por(bananeira(900.0), 900.0, 60.0);
            por(painel(980.0), 980.0, 60.0);
            por(barraca(1060.0, "c-urucum"), 1060.0, 108.0);
            extra.push_str(&abelha(340.0, 64.0, 1));
            extra.push_str(&abelha(880.0, 52.0, 2));
            extra.push_str(&abelha(620.0, 96.0, 3));
        }
    }
    d.push_str(&extra);
    (d, sombras)
}

/// Raio do círculo de luz do sol (unidades do viewBox) para uma escala.
pub fn raio_luz(escala: f64) -> f64 {
    150.0 * escala.clamp(0.6, 1.8)
}

/// Escala nova do sol a partir da velocidade do cursor (px/ms), suavizada.
pub fn escala_pela_velocidade(antiga: f64, velocidade: f64) -> f64 {
    let alvo = 1.0 + (velocidade * 0.6).min(0.8);
    antiga * 0.75 + alvo * 0.25
}

/// Posição de descanso do sol (sem cursor na faixa).
const DESCANSO: (f64, f64) = (1010.0, 74.0);

/// A faixa ilustrada da tela `nome` com o sol que segue o cursor.
#[component]
pub fn Faixa(nome: &'static str) -> impl IntoView {
    let (desenho, sombras) = cena(nome);
    let pos = RwSignal::new(DESCANSO);
    let escala = RwSignal::new(0.8_f64);
    let ativo = RwSignal::new(false);
    let ultimo = StoredValue::new((0.0_f64, 0.0_f64, 0.0_f64));
    let svg = NodeRef::<leptos::svg::Svg>::new();
    let mover = move |ev: leptos::ev::PointerEvent| {
        let Some(el) = svg.get() else { return };
        let el: &web_sys::Element = el.unchecked_ref();
        let r = el.get_bounding_client_rect();
        if r.width() <= 0.0 {
            return;
        }
        let x = (f64::from(ev.client_x()) - r.left()) / r.width() * W;
        let y = (f64::from(ev.client_y()) - r.top()) / r.height() * H;
        let t = js_sys::Date::now();
        let (lx, ly, lt) = ultimo.get_value();
        let dt = (t - lt).max(1.0);
        let v = ((x - lx).hypot(y - ly) / dt).min(5.0);
        ultimo.set_value((x, y, t));
        escala.update(|e| *e = escala_pela_velocidade(*e, v));
        pos.set((x.clamp(0.0, W), y.clamp(0.0, H)));
        ativo.set(true);
    };
    let sair = move |_| {
        ativo.set(false);
        pos.set(DESCANSO);
        escala.set(0.8);
    };
    // --sx: de −1 (sol à esquerda) a 1 (à direita): as sombras tombam para o outro lado.
    let estilo = move || {
        let (x, _) = pos.get();
        format!("--sx:{:.3}", (x / W) * 2.0 - 1.0)
    };
    let sol_t = move || {
        let (x, y) = pos.get();
        format!(
            "transform:translate({x:.1}px,{y:.1}px) scale({:.3})",
            escala.get()
        )
    };
    let luz_t = move || {
        let (x, y) = pos.get();
        format!(
            "transform:translate({x:.1}px,{y:.1}px) scale({:.3})",
            raio_luz(escala.get()) / 150.0
        )
    };
    view! {
        <div class="faixa-mutirao" class:acesa=move || ativo.get()>
            <svg node_ref=svg viewBox=format!("0 0 {W} {H}") preserveAspectRatio="xMidYMax slice" style=estilo
                on:pointermove=mover on:pointerleave=sair on:pointerdown=mover role="img"
                aria-label=format!("Ilustração xilogravada: {nome}, mutirão solar")>
                <defs>
                    <pattern id="mt-hach" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
                        <path d="M0 0 V6" stroke="#17130F" stroke-width="1.6" />
                    </pattern>
                    <pattern id="mt-goiva" width="18" height="10" patternUnits="userSpaceOnUse">
                        <path d="M0 6 q4 -4 9 0 t9 0" fill="none" stroke="#17130F" stroke-width="1.4" />
                    </pattern>
                    <radialGradient id="mt-gradluz">
                        <stop offset="0" stop-color="#fff" />
                        <stop offset=".7" stop-color="#fff" stop-opacity=".9" />
                        <stop offset="1" stop-color="#fff" stop-opacity="0" />
                    </radialGradient>
                    <mask id="mt-luz" maskUnits="userSpaceOnUse" x="0" y="0" width=W height=H>
                        <rect width=W height=H fill="#000" />
                        <circle class="luz" r="150" cx="0" cy="0" fill="url(#mt-gradluz)" style=luz_t />
                    </mask>
                </defs>
                <rect width=W height=H class="ceu" />
                <g class="sombras" inner_html=sombras></g>
                <g class="camada tinta" inner_html=desenho.clone()></g>
                <g class="camada cor" mask="url(#mt-luz)" inner_html=desenho></g>
                <g class="sol" style=sol_t>
                    <g class="raios">
                        {(0..16).map(|i| {
                            let a = f64::from(i) * 22.5;
                            let comprido = i % 2 == 0;
                            let d = if comprido { "M-6 -40 L0 -66 L6 -40 Z" } else { "M-4 -40 L0 -54 L4 -40 Z" };
                            view! { <path d=d transform=format!("rotate({a})") class="raio" /> }
                        }).collect_view()}
                    </g>
                    <circle r="34" class="disco" />
                    <path d="M-14 -6 q4 -5 8 0 M6 -6 q4 -5 8 0 M-12 10 q12 10 24 0" class="rosto" />
                    <circle cx="-17" cy="5" r="4" class="bochecha" /><circle cx="17" cy="5" r="4" class="bochecha" />
                </g>
            </svg>
        </div>
    }
}

pub const CSS: &str = r#"
.faixa-mutirao { margin:-24px -28px 14px; border-bottom:3px solid var(--tinta); overflow:hidden; border-radius:26px 26px 0 0; touch-action:pan-y; }
.faixa-mutirao svg { display:block; width:100%; height:auto; aspect-ratio:1200 / 240; max-height:250px; cursor:none; }
.faixa-mutirao .ceu { fill:var(--kraft); }
.faixa-mutirao .linha, .faixa-mutirao .rosto, .faixa-mutirao .helice, .faixa-mutirao .onda { fill:none; stroke:var(--tinta); stroke-width:2.4; stroke-linecap:round; stroke-linejoin:round; }
.faixa-mutirao .linha.grossa { stroke-width:3.6; }
.faixa-mutirao .linha.fina { stroke-width:1.6; }
.faixa-mutirao .camada * { stroke:var(--tinta); stroke-width:2.6; stroke-linejoin:round; stroke-linecap:round; }
.faixa-mutirao .t-hach { stroke:none !important; }
/* Camada de tinta: tudo papel cru, só o traço e as hachuras. */
.faixa-mutirao .tinta [class*="c-"] { fill:var(--kraft); }
.faixa-mutirao .tinta .c-morro { fill:url(#mt-hach); opacity:.35; }
/* Camada de cor: a feira acesa. */
.faixa-mutirao .cor .c-verde { fill:var(--bananeira); }
.faixa-mutirao .cor .c-sol { fill:var(--sol); }
.faixa-mutirao .cor .c-urucum { fill:var(--urucum); }
.faixa-mutirao .cor .c-azul { fill:var(--azulejo); }
.faixa-mutirao .cor .c-hib { fill:var(--hibisco); }
.faixa-mutirao .cor .c-kraft { fill:#F6EBD0; }
.faixa-mutirao .cor .c-taipa { fill:#C98B54; }
.faixa-mutirao .cor .c-taipa-2 { fill:#D9A066; }
.faixa-mutirao .cor .c-terra { fill:#8A5A34; }
.faixa-mutirao .cor .c-madeira { fill:#9C6B3F; }
.faixa-mutirao .cor .c-pele { fill:#B9774B; }
.faixa-mutirao .cor .c-agua { fill:#5AA7D8; }
.faixa-mutirao .cor .c-morro { fill:#7DB36A; }
.faixa-mutirao .sombras .sombra { fill:var(--tinta); opacity:.22; transform:skewX(calc(var(--sx) * -38deg)) translateX(calc(var(--sx) * -14px)); transition:transform .25s ease-out; }
/* O sol: disco, rosto de cordel e raios que giram. */
.faixa-mutirao .sol { transition:transform .5s cubic-bezier(.2,.8,.2,1); pointer-events:none; }
.faixa-mutirao.acesa .sol, .faixa-mutirao.acesa .luz { transition:transform .08s linear; }
.faixa-mutirao .luz { transition:transform .5s cubic-bezier(.2,.8,.2,1); }
.faixa-mutirao .disco { fill:var(--sol); stroke:var(--tinta); stroke-width:3.2; }
.faixa-mutirao .raio { fill:var(--sol); stroke:var(--tinta); stroke-width:2.4; stroke-linejoin:round; }
.faixa-mutirao .raios { animation:mt-girar 14s linear infinite; }
.faixa-mutirao.acesa .raios { animation-duration:5s; }
.faixa-mutirao .bochecha { fill:var(--urucum); opacity:.75; }
@keyframes mt-girar { to { transform:rotate(360deg); } }
/* Na luz, a cena acorda (as animações só existem na camada de cor). */
.faixa-mutirao .cor .flor, .faixa-mutirao .cor .fruta { transform-box:fill-box; transform-origin:center; animation:mt-flor 1.6s ease-in-out infinite alternate; }
@keyframes mt-flor { from { transform:scale(.7); } to { transform:scale(1.25); } }
.faixa-mutirao .cor .painel { animation:mt-painel 1.4s ease-in-out infinite alternate; }
@keyframes mt-painel { from { fill:var(--azulejo); } to { fill:#6FC3FF; } }
.faixa-mutirao .cor .lampada { animation:mt-lamp .8s ease-in-out infinite alternate; }
@keyframes mt-lamp { from { fill:var(--sol); } to { fill:#FFF3B0; } }
.faixa-mutirao .cor .roda { animation:mt-girar 1.2s linear infinite; }
.faixa-mutirao .cor .pas { animation:mt-girar 3s linear infinite; }
.faixa-mutirao .cor .acena { animation:mt-acena .6s ease-in-out infinite alternate; }
@keyframes mt-acena { from { transform:rotate(-20deg); } to { transform:rotate(25deg); } }
.faixa-mutirao .cor .folha { animation:mt-folha 2.4s ease-in-out infinite alternate; }
@keyframes mt-folha { from { transform:rotate(-4deg); } to { transform:rotate(5deg); } }
.faixa-mutirao .cor .chama { animation:mt-chama .5s ease-in-out infinite alternate; }
@keyframes mt-chama { from { transform:scaleY(.85); } to { transform:scaleY(1.15); } }
.faixa-mutirao .cor .onda { animation:mt-onda 1.6s ease-in-out infinite alternate; }
@keyframes mt-onda { to { transform:translateX(-8px); } }
.faixa-mutirao .bandeira { animation:mt-bandeira 2.2s ease-in-out infinite alternate; }
@keyframes mt-bandeira { from { transform:rotate(-6deg); } to { transform:rotate(6deg); } }
.faixa-mutirao .abelha { animation:mt-voo 4s ease-in-out infinite alternate; }
.faixa-mutirao .abelha.a2 { animation-duration:5.2s; animation-direction:alternate-reverse; }
.faixa-mutirao .abelha.a3 { animation-duration:3.4s; }
@keyframes mt-voo { 0% { transform:translate(0,0); } 50% { transform:translate(40px,-14px); } 100% { transform:translate(80px,6px); } }
.faixa-mutirao .cor .helice { animation:mt-helice .15s linear infinite alternate; }
@keyframes mt-helice { to { transform:scaleX(.3); } }

body.leve .faixa-mutirao .camada.cor { mask:none; }
"#;

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn sol_cresce_com_a_velocidade_e_tem_limite() {
        let parado = escala_pela_velocidade(1.0, 0.0);
        let rapido = escala_pela_velocidade(1.0, 4.0);
        assert!(rapido > parado);
        assert!(escala_pela_velocidade(1.8, 5.0) <= 1.8 + 1e-9);
        assert_eq!(raio_luz(9.0), 270.0);
        // Toda tela tem cena com peças e sombras.
        for t in [
            "Visão geral",
            "Caçada",
            "Combate",
            "Mercado",
            "Bestiário",
            "Automação",
        ] {
            let (d, s) = cena(t);
            assert!(d.contains("class=\"peca"), "{t}");
            assert!(s.contains("sombra"), "{t}");
        }
    }
}
