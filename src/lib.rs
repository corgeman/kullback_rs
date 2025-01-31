#![allow(clippy::needless_return)]

mod utils;

use wasm_bindgen::prelude::*;
use plotters::prelude::*;
use plotters_canvas::CanvasBackend;
use web_sys::HtmlCanvasElement;
use base64::{Engine as _, prelude::BASE64_STANDARD as b64};
use base16ct::mixed::decode_vec as hexdec;
use nohash_hasher::IntMap;

/*
TODO:
- If there is a consecutive chain of spikes (ex. periods 17,18,19 all 'spike'), then
  only place a triangle on the largest spike in that chain.

- If the period is very low (2, 3, 4), spikes are so frequent that they don't deviate from the mean
  and therefore our standard deviation test can't find them. Find some solution to fix this.

- The way error handling currently works can leave some vague error messages. For instance, 
  providing invalid base64 displays the error "Invalid input length" which is pretty unclear. 
  Something like "Base64: Invalid input length" would be much better.
*/



/// Analyze some text using the Kullback test and graph the results to a canvas.
/// 
/// `canvas` - an HTML Canvas element that will have the results graphed to it.
/// 
/// `input` - the input data to analyze.
/// 
/// `range` - the maximum period to check
/// 
/// `fmt` - the encoding of the data. Should be 'UTF8', 'BASE64', or 'HEX'.
/// 
/// `cache` - a list of previously-computed IoC calculations so they don't need to be recalculated
#[wasm_bindgen]
pub fn analyze(canvas: HtmlCanvasElement, input: &str, range: usize, fmt: &str, mut cache: Vec<f32>) -> Result<Vec<f32>,  JsValue> {
    utils::set_panic_hook();

    // If cache holds enough data to graph, use it!
    if range <= cache.len() {
        let cached: Vec<f32> = cache.clone().into_iter().take(range-1).collect();
        plot(canvas,&cached)?;
        return Ok(cache) 
    } // Otherwise we'll need to extend it down below.

    /*
    Decode the input from UTF8, hex, or base64.
    Kinda hacky since we're treating potentially binary data as characters,
    but we're only looking for a unique mapping here and UTF8 can map all byte values 0-255.
    */
    let data: Vec<char> = match fmt {
        "UTF8" => input.chars().collect(),
        "HEX" => hexdec(input).map_err(|e| e.to_string())?
                    .iter().map(|d| *d as char).collect(),
        "BASE64" => b64.decode(input).map_err(|e| e.to_string())?
                        .iter().map(|d| *d as char).collect(),
        _ => return Err("Invalid encoding format (???)".into())
    };

    // Don't analyze extremely short data.
    if data.len() < 4 {return Err("Length of input is too short (4 chars minimum)".into())}

    // Range cannot be greater than len(data)/2, because analyzing less than two full blocks is useless.
    // This will be limited client-side but I can't be 100% sure that works.
    if range > data.len()/2 {return Err("Range is too large. Please decrease.".into())}

    // Extend cache to hold everything we need
    cache.reserve(range-cache.len());

    // We also need to create a Vec that stores each transposition output.
    let mut transpositions = vec!['\u{0}'; data.len()+range];

    for i in (cache.len()+1)..range{
        let chunk_size = data.len().div_ceil(i);
        let t_len = chunk_size*i; // how much of the transpositions array we use
        transpose(&data,&mut transpositions,i);

        let cap = if (data.len() % i) != 0 {data.len() % i} else {i}; // # of complete chunks

        let mut sum: f32 = 0.0;
        for (j, x) in transpositions[..t_len].chunks_exact(chunk_size).enumerate() {
            // Every chunk past the 'cap'th needs to have its last character stripped off
            // as it is an 'incomplete' row. For instance, transposing 'xyzxyzxyzx' with n=3 gets:
            /*
            xxxx
            yyy\0
            zzz\0

            where \0 means 'null byte'.
            */
            // Here, the last two rows are incomplete and so their last character is "uninitialized".
            let fixed = &x[..chunk_size-((j >= cap) as usize)];
            sum += ioc(fixed);
        }
        cache.push(sum / i as f32);
    }

    // Plot it!
    plot(canvas,&cache)?;

    return Ok(cache);
}

/// Plot a list of IoC results given an HTML canvas.
/// 
/// `canvas` - an HTML Canvas element that will have the results graphed to it.
/// 
/// `iocs` - the list of IoC results that should be plotted.
fn plot(canvas: HtmlCanvasElement, iocs: &[f32]) -> Result<(), JsValue>{
    let range: usize = iocs.len()+1;

    // Calculate max/min/mean/standard deviation.
    let min = *iocs.iter().min_by(|x,y| x.total_cmp(y)).unwrap();
    let max = *iocs.iter().max_by(|x,y| x.total_cmp(y)).unwrap();
    let mean = iocs.iter().sum::<f32>() / iocs.len() as f32;
    let stdev = (iocs.iter().map(|x| (mean-x).powi(2)).sum::<f32>() / iocs.len() as f32).sqrt();

    // Find all IoC calculations 1.5 standard deviations from the mean
    let spikes: Vec<usize> = iocs.iter().enumerate().filter(|&(_, x)| ((x-mean)/stdev > 1.5)).map(|(i, _)| i).collect();

    // Begin plotting!
    let root = CanvasBackend::with_canvas_object(canvas)
    .ok_or("Failed to create canvas")?.into_drawing_area();
    root.fill(&WHITE).unwrap();

    let mut chart_builder = ChartBuilder::on(&root);
    chart_builder.margin(5).margin_top(15).set_left_and_bottom_label_area_size(35);
    let mut chart_context = chart_builder
        .build_cartesian_2d(0..range, (min*0.95)..(max*1.05))
        .map_err(|e| e.to_string())?;
    chart_context.configure_mesh()
    .disable_mesh().draw().map_err(|e| e.to_string())?; // sounds redundant but required for axes

    // Create a red/black triangle at (x,y)
    let triangle = |x: usize, y: f32|{
        return EmptyElement::at((x, y))
            + TriangleMarker::new((0, 0), 7, ShapeStyle::from(&BLACK))
            + TriangleMarker::new((0, 0), 5, ShapeStyle::from(&RED))
    };

    // Write x at (x,y)
    let label = |x: usize, y: f32| {
        return EmptyElement::at((x, y))
            + Text::new(
                x.to_string(),
                (10, 0),
                ("sans-serif", 15.0).into_font(),
            )
    };

    // Draw blue line on graph
    chart_context.draw_series(LineSeries::new(
        (1..range).zip(iocs.to_owned().clone().into_iter()),
        &BLUE,
    )).map_err(|e| e.to_string())?;

    // Draw triangle & label for each spike
    for i in spikes {
        chart_context.plotting_area().draw(&label(i+1, iocs[i]))
            .map_err(|e| e.to_string())?;
        chart_context.plotting_area().draw(&triangle(i+1, iocs[i]))
            .map_err(|e| e.to_string())?;
    }

    return Ok(());
}

/// Index of Coincidence calculator.
/// 
/// `data` - the data to perform the calculation on.
fn ioc(data: &[char]) -> f32 {
    /* HashMap that uses the character itself as the 'hash'.
       There is some concern about this here: https://www.reddit.com/r/rust/comments/ps6fzn/hasher_for_char_keys/
       but while I haven't done rigorous testing it seems WAY faster than [0usize; 256], my previous solution.
       This is because memset/memcpy seem fairly expensive in WebAssembly. */
    let mut counts: IntMap<u32,usize> = IntMap::default();
    let l = data.len() as f32;

    // Count frequencies of each character.
    for x in data {
        counts.entry(*x as u32).and_modify(|x| *x += 1).or_insert(1);
    }

    // Perform the actual IoC calculation. https://en.wikipedia.org/wiki/Index_of_coincidence
    return counts.into_values().map(|x| x*(x-1)).sum::<usize>() as f32 / (l*(l-1.0))
}


/// Transpose data into `n` rows. 
/// 
/// `data` - the data to be transposed.
/// 
/// `output` - the output buffer for transposition. Needs to be `n` bytes larger than `data`'s length to account for incomplete rows.
/// 
/// `n` - the number of rows the data should be transposed into.
fn transpose<T: Copy>(data: &[T], output: &mut[T], n: usize) {
    let chunk_size = data.len().div_ceil(n);
    for (i, d) in data.iter().enumerate() {
        output[((i%n)*(chunk_size))+(i/n)] = *d;
    }
}