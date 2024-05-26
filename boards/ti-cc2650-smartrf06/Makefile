# Makefile for building the tock kernel for the CC2650-SmartRF06

TOCK_BOARD=ti-cc2650-smartrf06
TARGET=thumbv7m-none-eabi
PLATFORM=ti-cc2650-smartrf06
OPENOCD ?= openocd

include ../Makefile.common

.PHONY: flash
flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(TOCK_BOARD).bin
	$(OPENOCD) -f flash-kernel.openocd