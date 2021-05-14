#include <stdint.h>
#include <stdlib.h>
#include <jpeglib.h>


typedef struct jpeg_compressor_t {
	struct jpeg_error_mgr error;
	struct jpeg_compress_struct compress;
} jpeg_compressor_t;


size_t size_min(size_t x, size_t y) {
	return x < y ? x : y;
}

jpeg_compressor_t* jpeg_compressor_create() {
	jpeg_compressor_t* self = malloc(sizeof(jpeg_compressor_t));
	self->compress.err = jpeg_std_error(&self->error);
	jpeg_create_compress(&self->compress);
	return self;
}

void jpeg_compressor_destroy(jpeg_compressor_t* self) {
	jpeg_destroy_compress(&self->compress);
	free(self);
}

size_t jpeg_compressor_compress(jpeg_compressor_t* self, uint8_t* dst, size_t dst_size, uint32_t const* src, size_t stride, size_t w, size_t h) {
	unsigned long dst_size_ul = dst_size;
	jpeg_mem_dest(&self->compress, &dst, &dst_size_ul);
	self->compress.image_width = w;
	self->compress.image_height = h;
	self->compress.input_components = 4;
	self->compress.in_color_space = JCS_EXT_BGRX;
	jpeg_set_defaults(&self->compress);
	// quality: 7-bit DC value.
	jpeg_set_quality(&self->compress, 93, TRUE);
	// 4:4:4 sampling.
	self->compress.comp_info[0].h_samp_factor = 1;
	self->compress.comp_info[1].h_samp_factor = 1;
	self->compress.comp_info[2].h_samp_factor = 1;
	self->compress.comp_info[0].v_samp_factor = 1;
	self->compress.comp_info[1].v_samp_factor = 1;
	self->compress.comp_info[2].v_samp_factor = 1;
#if 0
	// omit quantization tables.
	for (size_t i = 0; i < NUM_QUANT_TBLS; ++i) {
		JQUANT_TBL* tbl = self->compress.quant_tbl_ptrs[i];
		if (tbl != NULL) {
			tbl->sent_table = TRUE;
		}
	}
#endif
	// omit huffman tables.
	for (size_t i = 0; i < NUM_HUFF_TBLS; ++i) {
		JHUFF_TBL* tbl_dc = self->compress.dc_huff_tbl_ptrs[i];
		if (tbl_dc != NULL) {
			tbl_dc->sent_table = TRUE;
		}
		JHUFF_TBL* tbl_ac = self->compress.ac_huff_tbl_ptrs[i];
		if (tbl_ac != NULL) {
			tbl_ac->sent_table = TRUE;
		}
	}

	jpeg_start_compress(&self->compress, FALSE);
	while (self->compress.next_scanline < h) {
		size_t n = size_min(h - self->compress.next_scanline, 256);
		uint8_t* lines[256];
		for (size_t i = 0; i < n; ++i) {
			lines[i] = (uint8_t*)(src + stride * (self->compress.next_scanline + i));
		}
		jpeg_write_scanlines(&self->compress, lines, n);
	}
	jpeg_finish_compress(&self->compress);

	return dst_size_ul;
}
