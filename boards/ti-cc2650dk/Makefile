# Makefile for building the tock kernel for the CC2650DK

TOCK_BOARD=ti-cc2650dk
TARGET=thumbv7m-none-eabi
PLATFORM=ti-cc2650dk
OPENOCD ?= openocd

include ../Makefile.common

.PHONY: flash
flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(TOCK_BOARD).bin
	$(OPENOCD) -f flash-kernel.openocd