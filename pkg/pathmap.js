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

let WASM_VECTOR_LEN = 0;

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
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
 * @param {BytesTrieSet} bts
 * @returns {Array<any>}
 */
export function paths(bts) {
    _assertClass(bts, BytesTrieSet);
    const ret = wasm.paths(bts.__wbg_ptr);
    return ret;
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

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
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
 * @returns {BytesTrieSet}
 */
export function make_map(r) {
    _assertClass(r, Reader);
    const ret = wasm.make_map(r.__wbg_ptr);
    return BytesTrieSet.__wrap(ret);
}

const BytesTrieSetFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bytestrieset_free(ptr >>> 0, 1));

export class BytesTrieSet {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(BytesTrieSet.prototype);
        obj.__wbg_ptr = ptr;
        BytesTrieSetFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BytesTrieSetFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bytestrieset_free(ptr, 0);
    }
}

const ReaderFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_reader_free(ptr >>> 0, 1));

export class Reader {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(Reader.prototype);
        obj.__wbg_ptr = ptr;
        ReaderFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ReaderFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_reader_free(ptr, 0);
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
    imports.wbg.__wbg_buffer_61b7ce01341d7f88 = function(arg0) {
        const ret = arg0.buffer;
        return ret;
    };
    imports.wbg.__wbg_new_254fa9eac11932ae = function() {
        const ret = new Array();
        return ret;
    };
    imports.wbg.__wbg_new_3ff5b33b1ce712df = function(arg0) {
        const ret = new Uint8Array(arg0);
        return ret;
    };
    imports.wbg.__wbg_newwithbyteoffsetandlength_ba35896968751d91 = function(arg0, arg1, arg2) {
        const ret = new Uint8Array(arg0, arg1 >>> 0, arg2 >>> 0);
        return ret;
    };
    imports.wbg.__wbg_parse_161c68378e086ae1 = function() { return handleError(function (arg0, arg1) {
        const ret = JSON.parse(getStringFromWasm0(arg0, arg1));
        return ret;
    }, arguments) };
    imports.wbg.__wbg_push_6edad0df4b546b2c = function(arg0, arg1) {
        const ret = arg0.push(arg1);
        return ret;
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
