let wasm;

const cachedTextDecoder = (typeof TextDecoder !== 'undefined' ? new TextDecoder('utf-8', { ignoreBOM: true, fatal: true }) : { decode: () => { throw Error('TextDecoder not available') } } );

if (typeof TextDecoder !== 'undefined') { cachedTextDecoder.decode(); };

let cachedUint8ArrayMemory0 = null;

function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_export_2.set(idx, obj);
    return idx;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}
/**
 * @returns {BytesTrieSet}
 */
export function empty() {
    const ret = wasm.empty();
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {number} start
 * @param {number} stop
 * @param {number} step
 * @returns {BytesTrieSet}
 */
export function range_be_u32(start, stop, step) {
    const ret = wasm.range_be_u32(start, stop, step);
    return BytesTrieSet.__wrap(ret);
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}
/**
 * @param {BytesTrieSet} x
 * @param {BytesTrieSet} y
 * @returns {BytesTrieSet}
 */
export function union(x, y) {
    _assertClass(x, BytesTrieSet);
    _assertClass(y, BytesTrieSet);
    const ret = wasm.union(x.__wbg_ptr, y.__wbg_ptr);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} x
 * @param {BytesTrieSet} y
 * @returns {BytesTrieSet}
 */
export function intersection(x, y) {
    _assertClass(x, BytesTrieSet);
    _assertClass(y, BytesTrieSet);
    const ret = wasm.intersection(x.__wbg_ptr, y.__wbg_ptr);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} x
 * @param {BytesTrieSet} y
 * @returns {BytesTrieSet}
 */
export function restriction(x, y) {
    _assertClass(x, BytesTrieSet);
    _assertClass(y, BytesTrieSet);
    const ret = wasm.restriction(x.__wbg_ptr, y.__wbg_ptr);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} x
 * @param {BytesTrieSet} y
 * @returns {BytesTrieSet}
 */
export function subtraction(x, y) {
    _assertClass(x, BytesTrieSet);
    _assertClass(y, BytesTrieSet);
    const ret = wasm.subtraction(x.__wbg_ptr, y.__wbg_ptr);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} x
 * @param {BytesTrieSet} y
 * @returns {BytesTrieSet}
 */
export function raffination(x, y) {
    _assertClass(x, BytesTrieSet);
    _assertClass(y, BytesTrieSet);
    const ret = wasm.raffination(x.__wbg_ptr, y.__wbg_ptr);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} x
 * @param {number} k
 * @returns {BytesTrieSet}
 */
export function decapitation(x, k) {
    _assertClass(x, BytesTrieSet);
    const ret = wasm.decapitation(x.__wbg_ptr, k);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} x
 * @param {number} k
 * @returns {BytesTrieSet}
 */
export function head(x, k) {
    _assertClass(x, BytesTrieSet);
    const ret = wasm.head(x.__wbg_ptr, k);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} x
 * @param {BytesTrieSet} y
 * @returns {BytesTrieSet}
 */
export function product(x, y) {
    _assertClass(x, BytesTrieSet);
    _assertClass(y, BytesTrieSet);
    const ret = wasm.product(x.__wbg_ptr, y.__wbg_ptr);
    return BytesTrieSet.__wrap(ret);
}

let WASM_VECTOR_LEN = 0;

const cachedTextEncoder = (typeof TextEncoder !== 'undefined' ? new TextEncoder('utf-8') : { encode: () => { throw Error('TextEncoder not available') } } );

const encodeString = (typeof cachedTextEncoder.encodeInto === 'function'
    ? function (arg, view) {
    return cachedTextEncoder.encodeInto(arg, view);
}
    : function (arg, view) {
    const buf = cachedTextEncoder.encode(arg);
    view.set(buf);
    return {
        read: arg.length,
        written: buf.length
    };
});

function passStringToWasm0(arg, malloc, realloc) {

    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }

    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = encodeString(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}
/**
 * @param {BytesTrieSet} bts
 * @param {string} pattern
 * @param {string} template
 * @returns {BytesTrieSet}
 */
export function regex_transform(bts, pattern, template) {
    _assertClass(bts, BytesTrieSet);
    const ptr0 = passStringToWasm0(pattern, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(template, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.regex_transform(bts.__wbg_ptr, ptr0, len0, ptr1, len1);
    return BytesTrieSet.__wrap(ret);
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}
/**
 * @param {BytesTrieSet} x
 * @param {Uint8Array} path
 * @returns {BytesTrieSet}
 */
export function wrap(x, path) {
    _assertClass(x, BytesTrieSet);
    const ptr0 = passArray8ToWasm0(path, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.wrap(x.__wbg_ptr, ptr0, len0);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} m
 * @param {Uint8Array} k
 * @returns {boolean}
 */
export function contains(m, k) {
    _assertClass(m, BytesTrieSet);
    const ptr0 = passArray8ToWasm0(k, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.contains(m.__wbg_ptr, ptr0, len0);
    return ret !== 0;
}

/**
 * @param {Array<any>} paths
 * @returns {BytesTrieSet}
 */
export function from_paths(paths) {
    const ret = wasm.from_paths(paths);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} bts
 * @returns {object}
 */
export function object(bts) {
    _assertClass(bts, BytesTrieSet);
    const ret = wasm.object(bts.__wbg_ptr);
    return ret;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}
/**
 * @param {BytesTrieSet} bts
 * @returns {Uint8Array}
 */
export function serialize(bts) {
    _assertClass(bts, BytesTrieSet);
    const ret = wasm.serialize(bts.__wbg_ptr);
    var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v1;
}

/**
 * @param {Uint8Array} jsbs
 * @returns {BytesTrieSet}
 */
export function deserialize(jsbs) {
    const ret = wasm.deserialize(jsbs);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {BytesTrieSet} bts
 * @returns {object}
 */
export function d3_hierarchy(bts) {
    _assertClass(bts, BytesTrieSet);
    const ret = wasm.d3_hierarchy(bts.__wbg_ptr);
    return ret;
}

/**
 * @param {BytesTrieSet} bts
 * @returns {Reader}
 */
export function reader(bts) {
    _assertClass(bts, BytesTrieSet);
    const ret = wasm.reader(bts.__wbg_ptr);
    return Reader.__wrap(ret);
}

/**
 * @param {Reader} r
 * @param {Uint8Array} k
 * @returns {boolean}
 */
export function descend_to(r, k) {
    _assertClass(r, Reader);
    const ptr0 = passArray8ToWasm0(k, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.descend_to(r.__wbg_ptr, ptr0, len0);
    return ret !== 0;
}

/**
 * @param {Reader} r
 * @returns {boolean}
 */
export function to_next_val(r) {
    _assertClass(r, Reader);
    const ret = wasm.to_next_val(r.__wbg_ptr);
    return ret !== 0;
}

/**
 * @param {Reader} r
 * @param {number} i
 * @returns {boolean}
 */
export function descend_indexed_byte(r, i) {
    _assertClass(r, Reader);
    const ret = wasm.descend_indexed_byte(r.__wbg_ptr, i);
    return ret !== 0;
}

/**
 * @param {Reader} r
 * @returns {boolean}
 */
export function to_next_sibling_byte(r) {
    _assertClass(r, Reader);
    const ret = wasm.to_next_sibling_byte(r.__wbg_ptr);
    return ret !== 0;
}

/**
 * @param {Reader} r
 * @returns {boolean}
 */
export function to_prev_sibling_byte(r) {
    _assertClass(r, Reader);
    const ret = wasm.to_prev_sibling_byte(r.__wbg_ptr);
    return ret !== 0;
}

/**
 * @param {Reader} r
 * @param {number} k
 * @returns {boolean}
 */
export function ascend(r, k) {
    _assertClass(r, Reader);
    const ret = wasm.ascend(r.__wbg_ptr, k);
    return ret !== 0;
}

/**
 * @param {Reader} r
 * @returns {boolean}
 */
export function exists(r) {
    _assertClass(r, Reader);
    const ret = wasm.exists(r.__wbg_ptr);
    return ret !== 0;
}

/**
 * @param {Reader} r
 * @returns {Uint8Array}
 */
export function children(r) {
    _assertClass(r, Reader);
    const ret = wasm.children(r.__wbg_ptr);
    var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v1;
}

/**
 * @param {Reader} r
 * @returns {Uint8Array}
 */
export function path(r) {
    _assertClass(r, Reader);
    const ret = wasm.path(r.__wbg_ptr);
    var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v1;
}

/**
 * @param {Reader} r
 * @returns {Array<any>}
 */
export function read_paths(r) {
    _assertClass(r, Reader);
    const ret = wasm.read_paths(r.__wbg_ptr);
    return ret;
}

/**
 * @param {Reader} r
 * @param {number} value_limit
 * @param {number} byte_limit
 * @returns {Array<any>}
 */
export function traverse_paths(r, value_limit, byte_limit) {
    _assertClass(r, Reader);
    const ret = wasm.traverse_paths(r.__wbg_ptr, value_limit, byte_limit);
    return ret;
}

/**
 * @param {Reader} r
 * @returns {Uint8Array}
 */
export function min_path(r) {
    _assertClass(r, Reader);
    const ret = wasm.min_path(r.__wbg_ptr);
    var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v1;
}

/**
 * @param {Reader} r
 * @returns {Uint8Array}
 */
export function max_path(r) {
    _assertClass(r, Reader);
    const ret = wasm.max_path(r.__wbg_ptr);
    var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    return v1;
}

/**
 * @param {Reader} r
 * @returns {BytesTrieSet}
 */
export function make_map(r) {
    _assertClass(r, Reader);
    const ret = wasm.make_map(r.__wbg_ptr);
    return BytesTrieSet.__wrap(ret);
}

/**
 * @param {Reader} r
 * @returns {number}
 */
export function val_count(r) {
    _assertClass(r, Reader);
    const ret = wasm.val_count(r.__wbg_ptr);
    return ret >>> 0;
}

/**
 * @param {Reader} r
 * @returns {Reader}
 */
export function fork_reader(r) {
    _assertClass(r, Reader);
    const ret = wasm.fork_reader(r.__wbg_ptr);
    return Reader.__wrap(ret);
}

export class BytesTrieSet {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(BytesTrieSet.prototype);
        obj.__wbg_ptr = ptr;
        return obj;
    }
}

export class Reader {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(Reader.prototype);
        obj.__wbg_ptr = ptr;
        return obj;
    }
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);

            } catch (e) {
                if (module.headers.get('Content-Type') != 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else {
                    throw e;
                }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);

    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };

        } else {
            return instance;
        }
    }
}

function __wbg_get_imports() {
    const imports = {};
    imports.wbg = {};
    imports.wbg.__wbg_buffer_609cc3eee51ed158 = function(arg0) {
        const ret = arg0.buffer;
        return ret;
    };
    imports.wbg.__wbg_get_b9b93047fe3cf45b = function(arg0, arg1) {
        const ret = arg0[arg1 >>> 0];
        return ret;
    };
    imports.wbg.__wbg_length_a446193dc22c12f8 = function(arg0) {
        const ret = arg0.length;
        return ret;
    };
    imports.wbg.__wbg_length_e2d2a49132c1b256 = function(arg0) {
        const ret = arg0.length;
        return ret;
    };
    imports.wbg.__wbg_new_78feb108b6472713 = function() {
        const ret = new Array();
        return ret;
    };
    imports.wbg.__wbg_new_a12002a7f91c75be = function(arg0) {
        const ret = new Uint8Array(arg0);
        return ret;
    };
    imports.wbg.__wbg_newwithbyteoffsetandlength_d97e637ebe145a9a = function(arg0, arg1, arg2) {
        const ret = new Uint8Array(arg0, arg1 >>> 0, arg2 >>> 0);
        return ret;
    };
    imports.wbg.__wbg_parse_def2e24ef1252aff = function() { return handleError(function (arg0, arg1) {
        const ret = JSON.parse(getStringFromWasm0(arg0, arg1));
        return ret;
    }, arguments) };
    imports.wbg.__wbg_push_737cfc8c1432c2c6 = function(arg0, arg1) {
        const ret = arg0.push(arg1);
        return ret;
    };
    imports.wbg.__wbg_set_65595bdd868b3009 = function(arg0, arg1, arg2) {
        arg0.set(arg1, arg2 >>> 0);
    };
    imports.wbg.__wbindgen_init_externref_table = function() {
        const table = wasm.__wbindgen_export_2;
        const offset = table.grow(4);
        table.set(0, undefined);
        table.set(offset + 0, undefined);
        table.set(offset + 1, null);
        table.set(offset + 2, true);
        table.set(offset + 3, false);
        ;
    };
    imports.wbg.__wbindgen_memory = function() {
        const ret = wasm.memory;
        return ret;
    };
    imports.wbg.__wbindgen_throw = function(arg0, arg1) {
        throw new Error(getStringFromWasm0(arg0, arg1));
    };

    return imports;
}

function __wbg_init_memory(imports, memory) {

}

function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    __wbg_init.__wbindgen_wasm_module = module;
    cachedUint8ArrayMemory0 = null;


    wasm.__wbindgen_start();
    return wasm;
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (typeof module !== 'undefined') {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();

    __wbg_init_memory(imports);

    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }

    const instance = new WebAssembly.Instance(module, imports);

    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (typeof module_or_path !== 'undefined') {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (typeof module_or_path === 'undefined') {
        module_or_path = new URL('pathmap_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    __wbg_init_memory(imports);

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync };
export default __wbg_init;
