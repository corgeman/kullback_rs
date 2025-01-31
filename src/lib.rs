mod utils;

use wasm_bindgen::prelude::*;
use plotters::prelude::*;
use plotters_canvas::CanvasBackend;
use web_sys::HtmlCanvasElement;
use base64::{Engine as _, prelude::BASE64_STANDARD as b64};
use base16ct::mixed::decode_vec as hexdec;
use std::collections::HashMap;

/*
TODO:

- If there is a consecutive chain of spikes (ex. periods 17,18,19 all 'spike'), then
  only place a triangle on the largest spike in that chain.

- Profiling shows that wasm.memset() and wasm.memcpy() both take a significant amount
  of execution time. Find ways to reduce, if possible.
    - fast_ioc requires 1024 bytes (256*4) of zero'd out space every time it's called.
      Might be worth to write the counts in to a mutable 'scratch_space' argument, then during the
      filter_map zero out non-zero indices in that argument.

- If the period is very low (2, 3, 4), spikes are so frequent that they don't deviate from the mean
  and therefore our standard deviation test can't find them.
*/


#[wasm_bindgen]
pub fn analyze(canvas: HtmlCanvasElement, input: &str, range: usize, fmt: &str, mut cache: Vec<f32>) -> Result<Vec<f32>,  JsValue> {
    utils::set_panic_hook();

    // If cache holds enough data to graph, use it!
    if range <= cache.len() {
        let cached = cache.clone().into_iter().take(range-1).collect();
        plot(canvas,cached);
        return Ok(cache) 
    } // Otherwise we'll need to extend it down below.
   

    /*
    Decode the input from UTF8, hex, or base64.
    Kinda hacky since we're treating potentially binary data as characters,
    but we're only looking for a unique mapping here and UTF8 can map all byte values 0-255.

    Also, check if the data can be treated as bytes. We can use that to speed up the IoC calculation.
    */
    let (is_bytes, data) : (bool, Vec<char>) = match fmt {
        "UTF8" => (input.is_ascii() , input.chars().collect()),
        "HEX" => (true, hexdec(input).map_err(|e| e.to_string())?
                    .iter().map(|d| *d as char).collect()),
        "BASE64" => (true, b64.decode(input).map_err(|e| e.to_string())?
                        .iter().map(|d| *d as char).collect()),
        _ => return Err("Invalid encoding format (???)".into())
    };

    // Don't analyze extremely short data.
    if data.len() < 4 {return Err("Length of input is too short (4 chars minimum)".into())}

    // Range cannot be greater than len(data)/2, because analyzing less than two full blocks is useless.
    // This will be limited client-side but I can't be 100% sure that works.
    if range > data.len()/2 {return Err("Range is too large. Please decrease.".into())}

    // Extend cache to hold everything we need
    cache.reserve(range-cache.len());

    // We should also create a Vec that stores each transposition output.
    let mut transpositions = vec!['\u{0}'; data.len()+range];

    // keeping for testing: ioc calculation with transpose()
    // for i in (cache.len()+1)..range {
    //     let transpositions = transpose(&data,i);
    //     cache.push(transpositions.iter()
    //     .map(|x| if is_bytes {fast_ioc(x)} else {slow_ioc(x)})
    //     .sum::<f32>() / transpositions.len() as f32);
    // }

    for i in (cache.len()+1)..range{
        let chunk_size = data.len().div_ceil(i);
        let t_len = chunk_size*i; // how much of the transpositions array we use
        transpose4(&data,&mut transpositions,i); // Create transpositions
        // number of chunks with padding
        let cap = if (data.len() % i) != 0 {data.len() % i} else {i}; 

        let mut sum: f32 = 0.0;
        for (j, x) in transpositions[..t_len].chunks_exact(chunk_size).enumerate() {
            let fixed = &x[..chunk_size-((j >= cap) as usize)];
            sum += if is_bytes {fast_ioc(fixed)} else {slow_ioc(fixed)}
        }
        cache.push(sum / i as f32);
    }

    // Plot it!
    return plot(canvas,cache);
}

/// Plot a list of IoC results given an HTML canvas.
fn plot(canvas: HtmlCanvasElement, iocs: Vec<f32>) -> Result<Vec<f32>, JsValue>{
    let range: usize = iocs.len()+1;

    // Calculate max/min/mean/standard deviation.
    let min = *iocs.iter().min_by(|x,y| x.total_cmp(y)).unwrap();
    let max = *iocs.iter().max_by(|x,y| x.total_cmp(y)).unwrap();
    let mean = iocs.iter().sum::<f32>() / iocs.len() as f32;
    let stdev = (iocs.iter().map(|x| (mean-x).powi(2)).sum::<f32>() / iocs.len() as f32).sqrt();

    // Find all IoC calculations 1.5 standard deviations from the mean
    let spikes: Vec<usize> = iocs.iter().enumerate().filter_map(|(i, x)| ((x-mean)/stdev > 1.5).then(|| i)).collect();

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
        (1..range).zip(iocs.clone().into_iter()),
        &BLUE,
    )).map_err(|e| e.to_string())?;

    // Draw triangle & label for each spike
    for i in spikes {
        chart_context.plotting_area().draw(&label(i+1, iocs[i]))
            .map_err(|e| e.to_string())?;
        chart_context.plotting_area().draw(&triangle(i+1, iocs[i]))
            .map_err(|e| e.to_string())?;
    }

    return Ok(iocs);
}

/// IOC calculator without hashmap usage, used if input is all 0-255.
fn fast_ioc(data: &[char]) -> f32 {
    let mut counts = [0usize; 256];
    let l = data.len() as f32;

    // frequency count
    data.iter().for_each(|x| counts[*x as usize] += 1);

    /*
    Calculate the IoC.
    This code is technically problematic because if x is 0, (x-1) will underflow.
    However, since that gets multiplied by x, the underflow ends up doing nothing.
    A filter_map() could easily fix this, but I can't read WebAssembly so I don't know 
    if it would impact speed.
    */
    counts.into_iter().filter_map(|x| (x!=0).then(|| x*(x-1))).sum::<usize>() as f32 / (l*(l-1.0))
}

/// IOC calculator for text containing UTF8 characters.
fn slow_ioc(data: &[char]) -> f32 {
    let mut counts: HashMap<&char,usize> = HashMap::new();
    let l = data.len() as f32;

    for x in data {
        counts.entry(x).and_modify(|x| *x += 1).or_insert(1);
    }
    counts.into_values().map(|x| x*(x-1)).sum::<usize>() as f32 / (l*(l-1.0))
}


/*
This was my first attempt. Very ugly. Keeping to remember.
*/
/// Split a Vec into chunks of n size, then read those chunks by columns.
#[allow(dead_code)]
fn transpose<T: Copy>(data: &Vec<T>, n: usize) -> Box<[Vec<T>]>{
    let mut arr: Box<[Vec<T>]> = std::iter::repeat_with(|| 
        Vec::with_capacity(data.len().div_ceil(n)))
        .take(n)
        .collect(); 
    for (i, d) in data.iter().enumerate(){
        arr[i%n].push(*d);
    }
    arr
}

/*
Second attempt at writing transpose(). This one places the data all into a single Vec.
While better, it's still pretty gross because it causes data loss.
I don't use this because it (currently) causes some data loss. Specifically, the last data.len()%n characters get ignored.
This happened because I wasn't sure how to deal with them. You're supposed to call .chunks_exact() on the output of this function,
and each chunk is a single column of the transposition. That means each chunk has to the the same length.
So what should I do if the data is 'xyzxyzxyzxyzx' and n is 3? It would tranpose to this:
xxxxx
yyyy
zzzz
The first line is longer than the other two! That's no good. This function solves the problem by removing
the last 'x' from the input, making all lines equal-length. But again, data loss is not a good solution.

Alternatively, we could pad the last two lines to make them as long as the first, then have analyze()
strip off the padding, but what would we pad it with? 
An unmapped UTF8 codepoint? Return Vec<Option<char>> instead? Both of those felt *really* dumb, so I didn't do it.
*/
#[allow(dead_code)]
fn transpose2(data: &Vec<char>, n: usize) -> Vec<char> {
    let mut arr: Vec<char> = vec!['?'; data.len()-data.len()%n]; // (data.len()/n)*n) works but that looks dumb
    let chunk_size = data.len()/n;
    for (i, d) in data.iter().take(arr.len()).enumerate() {
        arr[((i*chunk_size)%(chunk_size*n))+(i/n)] = *d;
    }
    arr
}

/*
Solution number three to the transposition problem.
Continuing from transpose2(), it finally hit me that analyze() can already 
know which chunks will have padding or not-- it can calculate data.len() % n,
and that number is how many chunks will be padding-less. 

Therefore, we just pad with \0, and analyze() strips it off without even having
to look for it.
*/
#[allow(dead_code)]
fn transpose3(data: &Vec<char>, n: usize) -> Vec<char> {
    let chunk_size = data.len().div_ceil(n);
    let mut arr: Vec<char> = vec!['\u{0}'; chunk_size*n];
    for (i, d) in data.iter().enumerate() {
        arr[((i*chunk_size)%(chunk_size*n))+(i/n)] = *d;
    }
    arr
}


/*
Hopefully final iteration of the transposition problem. 
Creating and returning a Vec on EVERY transposition is pretty expensive,
especially if the input is massive. Instead, just write it to a given output buffer.
*/
fn transpose4(data: &Vec<char>, output: &mut Vec<char>, n: usize) {
    let chunk_size = data.len().div_ceil(n);
    for (i, d) in data.iter().enumerate() {
        output[((i*chunk_size)%(chunk_size*n))+(i/n)] = *d;
    }
}