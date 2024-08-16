package main

import (
	"C"
	"runtime/cgo"
	"unsafe"

	"github.com/penumbra-zone/reindexer/go/store"
)

//export c_store_new
func c_store_new(dir_ptr *C.char, dir_len C.int) unsafe.Pointer {
	dir := C.GoStringN(dir_ptr, dir_len)
	return unsafe.Pointer(uintptr(cgo.NewHandle(store.NewStore(dir))))
}

//export c_store_message_a
func c_store_message_a(ptr unsafe.Pointer) {
	cgo.Handle(uintptr(ptr)).Value().(*store.Store).MessageA()
}

//export c_store_message_b
func c_store_message_b(ptr unsafe.Pointer) {
	cgo.Handle(uintptr(ptr)).Value().(*store.Store).MessageB()
}

//export c_store_delete
func c_store_delete(ptr unsafe.Pointer) {
	cgo.Handle(uintptr(ptr)).Delete()
}

func main() {}
