_estack = 0x20100000;

MEMORY
{
	FLASH (rx)  : ORIGIN = 0x01081000, LENGTH = 0xF00000
	SRAM (rwx)  : ORIGIN = 0x20000000, LENGTH = 1024K
}

__FLASH_segment_start__ = 0x01000000;
__FLASH_segment_end__ = 0x02000000;
__SRAM_segment_start__ = ORIGIN(SRAM);
__SRAM_segment_end__ = ORIGIN(SRAM) + LENGTH(SRAM);

/* ARMv7-M MPU regions require power-of-two sizes and matching alignment. */
__privileged_functions_start__ = 0x01080000;
__privileged_functions_end__ = 0x010A0000;
__privileged_data_start__ = ORIGIN(SRAM);
__privileged_data_end__ = ORIGIN(SRAM) + 0x00002000;

SECTIONS
{
	.interrupts 0x01081000 :
	{
		. = ALIGN(4);
		KEEP(*(.isr_vector))
	} > FLASH

	.privileged_functions 0x01082000 :
	{
		KEEP(*(.fixSection))
		*(privileged_functions)
		. = __privileged_functions_end__ - 0x01082000;
	} > FLASH

	.text __privileged_functions_end__ :
	{
		. = 0x0;
		_stext = .;
		__syscalls_flash_start__ = .;
		*(freertos_system_calls)
		__syscalls_flash_end__ = .;
		*(.text*)
		*(.rodata*)
		_etext = .;
	} > FLASH

	.ARM.exidx :
	{
		__exidx_start = .;
		*(.ARM.exidx*)
		*(.gnu.linkonce.armexidx.*)
		__exidx_end = .;
	} > FLASH

	_sidata = .;
	.privileged_data : AT(_sidata)
	{
		_sdata = .;
		*(privileged_data)
		. = __privileged_data_end__ - __privileged_data_start__;
	} > SRAM
	ASSERT(ADDR(.privileged_data) == __privileged_data_start__, "FreeRTOS privileged data must start at the MPU region base")
	ASSERT((ADDR(.privileged_data) + SIZEOF(.privileged_data)) == __privileged_data_end__, "FreeRTOS privileged data must fill the MPU region")

	.data : AT(_sidata + SIZEOF(.privileged_data))
	{
		*(.data)
		*(.data.*)
		*(.gnu.linkonce.d.*)
		_edata = .;
	} > SRAM

    .stack :
    {
        . = ALIGN(4);
        _sstack = .;
        . = . + 0x2000;
        . = ALIGN(4);
        _estack = .;
    } > SRAM

	.freertos_heap (NOLOAD) :
	{
		. = ALIGN(8);
		*(.freertos_heap)
		*(.freertos_heap_probe)
		. = ALIGN(8);
	} > SRAM

	.mpu_sandbox_stack (NOLOAD) :
	{
		. = ALIGN(4096);
		*(.mpu_sandbox_stack)
	} > SRAM

	.mpu_sandbox_input (NOLOAD) :
	{
		. = ALIGN(256);
		*(.mpu_sandbox_input)
	} > SRAM

	.mpu_sandbox_output (NOLOAD) :
	{
		. = ALIGN(256);
		*(.mpu_sandbox_output)
	} > SRAM

	.mpu_sandbox_arena (NOLOAD) :
	{
		. = ALIGN(256);
		*(.mpu_sandbox_arena)
	} > SRAM

	.bss :
	{
		_sbss = .;
		*(.bss)
		*(.bss.*)
		*(.gnu.linkonce.b.*)
		*(COMMON)
		_ebss = .;
	} > SRAM
	ASSERT(_ebss <= 0x200FC000, "SRAM sections overlap the data parser reservation")

	.data_parser_section 0x200FC000 (NOLOAD) :
	{
		__data_parser_start = .;
		*(.data_parser_section)
		. = ALIGN(4);
		__data_parser_end = .;
	} > SRAM
	ASSERT(__data_parser_end <= __SRAM_segment_end__, "Data parser reservation exceeds SRAM")
	
	
	/DISCARD/ :
	{
		libc.a (*)
		libm.a (*)
		libgcc.a (*)
	}
}

end = .;
