// ####### scif_framework.h

use core::sync::atomic::{AtomicBool, Ordering};

use tock_cells::{map_cell::MapCell, optional_cell::OptionalCell, volatile_cell::VolatileCell};

use crate::driverlib;

pub(crate) struct Scif {
    pub(crate) aon_rtc: cc2650::AON_RTC,
    pub(crate) aon_wuc: cc2650::AON_WUC,
    pub(crate) aux_aiodio0: cc2650::AUX_AIODIO0,
    pub(crate) aux_aiodio1: cc2650::AUX_AIODIO1,
    pub(crate) aux_evctl: cc2650::AUX_EVCTL,
    pub(crate) aux_sce: cc2650::AUX_SCE,
    pub(crate) aux_timer: cc2650::AUX_TIMER,
    pub(crate) aux_wuc: cc2650::AUX_WUC,

    /// Driver internal data (located in MCU domain RAM, not shared with the Sensor Controller)
    scif_data: MapCell<SCIFData>,
    last_aux_ram_image: OptionalCell<&'static [u16]>,
}

static SCIF_READY: AtomicBool = AtomicBool::new(false);

// This is a hack. Rust does not allow creating references to packed structs,
// BUT I know that all my packed structs contain only u16s and are aligned,
// so references to any of their fields are aligned as well.
macro_rules! safe_packed_ref {
    ($place:expr) => {
        core::ptr::addr_of!($place).as_ref().unwrap_unchecked()
    };
}
pub(crate) use safe_packed_ref;

impl Scif {
    #[track_caller]
    fn scif_data(&self) -> SCIFData {
        self.scif_data.get().unwrap()
    }

    pub(crate) fn new(
        aon_rtc: cc2650::AON_RTC,
        aon_wuc: cc2650::AON_WUC,
        aux_aiodio0: cc2650::AUX_AIODIO0,
        aux_aiodio1: cc2650::AUX_AIODIO1,
        aux_evctl: cc2650::AUX_EVCTL,
        aux_sce: cc2650::AUX_SCE,
        aux_timer: cc2650::AUX_TIMER,
        aux_wuc: cc2650::AUX_WUC,
    ) -> Self {
        let last_aux_ram_image = OptionalCell::empty();
        let scif_data = MapCell::empty();

        Self {
            aon_rtc,
            aon_wuc,
            aux_aiodio0,
            aux_aiodio1,
            aux_evctl,
            aux_sce,
            aux_timer,
            aux_wuc,
            scif_data,
            last_aux_ram_image,
        }
    }

    /** \brief Initializes the driver
     *
     * This function must be called to enable the driver for operation. The function:
     * - Verifies that the driver is not already active
     * - Stores a local copy of the driver setup data structure, \ref self.scif_data_T
     * - Configures AUX domain hardware modules for operation (general and setup-specific). This includes
     *   complete I/O setup for all Sensor Controller tasks.
     * - Loads the generated AUX RAM image into the AUX RAM
     * - Initializes handshaking mechanisms for control, alert interrupt generation and data exchange
     * - Configures use of AUX domain wake-up sources
     * - Starts the Sensor Controller
     *
     * \param[in]      *pScifDriverSetup
     *     Driver setup, containing all relevant pointers and parameters for operation
     *
     * \return
     *     \ref SCIF_SUCCESS if successful, or \ref SCIF_ILLEGAL_OPERATION if the Sensor Controller already
     *     is active. The function call has no effect if unsuccessful.
     */
    pub(crate) unsafe fn scif_init(&self, scif_driver_setup: SCIFData) -> SCIFResult {
        // Perform sanity checks: The Sensor Controller cannot already be active
        if self.aon_wuc.auxctl.read().sce_run_en().bit_is_set() {
            return SCIFResult::IllegalOperation;
        }

        // Copy the driver setup
        self.scif_data.put(scif_driver_setup);

        // Enable clock for required AUX modules
        driverlib::AUXWUCClockEnable(
            driverlib::AUX_WUC_SMPH_CLOCK
                | driverlib::AUX_WUC_AIODIO0_CLOCK
                | driverlib::AUX_WUC_AIODIO1_CLOCK
                | driverlib::AUX_WUC_TIMER_CLOCK
                | driverlib::AUX_WUC_ANAIF_CLOCK
                | driverlib::AUX_WUC_TDCIF_CLOCK
                | driverlib::AUX_WUC_ADI_CLOCK
                | driverlib::AUX_WUC_OSCCTRL_CLOCK,
        );

        // Open the AUX I/O latches, which have undefined value after power-up. AUX_AIODIO will by default
        // drive '0' on all I/O pins, so AUX_AIODIO must be configured before IOC
        // FIXME: static inline fn
        driverlib::AUXWUCFreezeDisable();

        let scif_data = self.scif_data();

        // Upload the AUX RAM image
        if self.last_aux_ram_image.get() != Some(scif_data.aux_ram_image) {
            core::ptr::copy_nonoverlapping(
                scif_data.aux_ram_image.as_ptr() as *const u8,
                driverlib::AUX_RAM_BASE as *mut u8,
                scif_data.aux_ram_image.len() * core::mem::size_of::<u16>(),
            );
            self.last_aux_ram_image.set(scif_data.aux_ram_image);
        }

        // Perform task resource initialization
        (self.scif_data().fptr_task_resource_init)(self);

        // Map events to the Sensor Controller's vector table, and set reset vector = AON wakeup
        self.aux_evctl.veccfg0.write(|w| {
            w.vec0_ev()
                .aon_sw()
                .vec0_en()
                .en()
                .vec1_ev()
                .aon_rtc_ch2()
                .vec1_en()
                .en()
        });
        self.aux_evctl
            .veccfg1
            .write(|w| w.vec2_ev().aon_sw().vec3_ev().aon_sw().vec3_en().en());
        self.aux_sce.ctl.write(|w| w.reset_vector().bits(0x1));

        // Clear any vector flags currently set (due to previous hardware or SCIF driver operation)
        self.aux_evctl.vecflags.write(|w| w.bits(0));

        // Set the READY event
        self.aux_evctl.swevset.write(|w| w.swev0().set_bit());

        // Let AUX be powered down (clocks disabled, full retention) and the bus connection between the AUX
        // and MCU domains be disconnected by default. This may have been done already by the operating
        // system to be able to a framework dependencies on whether or not the Sensor Controller is used.
        driverlib::AUXWUCPowerCtrl(driverlib::AUX_WUC_POWER_DOWN);

        // Start the Sensor Controller, but first read a random register from the AUX domain to ensure
        // that the last write accesses have been completed
        self.aux_wuc.mcubusctl.read();
        driverlib::AONWUCAuxImageValid();
        driverlib::SysCtrlAonSync();

        // Register and enable the interrupts. If warm, we probably have task ALERT event(s) pending, which
        // will be triggered immediately. We need to clear the interrupts because they might have been used
        // previously
        //osalRegisterCtrlReadyInt();
        Self::osal_clear_ctrl_ready_int();
        Self::osal_enable_ctrl_ready_int();
        //osalRegisterTaskAlertInt();
        Self::osal_clear_task_alert_int();
        Self::osal_enable_task_alert_int();

        SCIFResult::Success
    } // scifInit

    /** \brief Uninitializes the driver in order to release hardware resources or switch to another driver
     *
     * All Sensor Controller tasks must have been stopped before calling this function, to a leaving
     * hardware modules used in an unknown state. Also, any tick generation must have been stopped to a
     * leaving handshaking with the tick source in an unknown state.
     *
     * This function will wait until the Sensor Controller is sleeping before shutting it down.
     */
    unsafe fn scif_uninit(&self) {
        // Wait until the Sensor Controller is idle (it might still be running, though not for long)
        while self.aux_sce.cpustat.read().sleep().bit_is_clear() {}

        // Stop and reset the Sensor Controller Engine
        self.aux_sce.ctl.write(|w| w.restart().set_bit());
        self.aux_sce
            .ctl
            .write(|w| w.restart().set_bit().suspend().set_bit());
        self.aux_sce.ctl.read();
        driverlib::AONWUCAuxImageInvalid();
        driverlib::SysCtrlAonSync();
        self.aux_sce.ctl.write(|w| w.bits(0));

        // Disable interrupts
        Self::osal_disable_ctrl_ready_int();
        Self::osal_disable_task_alert_int();

        // Perform task resource uninitialization
        (self.scif_data().fptr_task_resource_uninit)(self);

        // Disable clocks
        driverlib::AUXWUCClockDisable(
            driverlib::AUX_WUC_SMPH_CLOCK
                | driverlib::AUX_WUC_AIODIO0_CLOCK
                | driverlib::AUX_WUC_AIODIO1_CLOCK
                | driverlib::AUX_WUC_TIMER_CLOCK
                | driverlib::AUX_WUC_ANAIF_CLOCK
                | driverlib::AUX_WUC_TDCIF_CLOCK
                | driverlib::AUX_WUC_ADI_CLOCK,
        );
    } // scifUninit

    /** \brief Initializes a single I/O pin for Sensor Controller usage
     *
     * This function must be called for each I/O pin to be used after AUX I/O latching has been set
     * transparent. It configures (in the indicated order):
     * - AIODIO:
     *     - IOMODE
     *     - GPIODOUT
     *     - GPIODIE
     * - IOC:
     *     -IOCFGn (index remapped using \ref scifData.pAuxIoIndexToMcuIocfgOffsetLut[])
     *
     * \param[in]      auxIoIndex
     *     Index of the I/O pin, 0-15, using AUX mapping
     * \param[in]      ioMode
     *     Pin I/O mode, one of the following:
     *     - \ref AUXIOMODE_OUTPUT
     *     - \ref AUXIOMODE_INPUT
     *     - \ref AUXIOMODE_OPEN_DRAIN
     *     - \ref AUXIOMODE_OPEN_DRAIN_WITH_INPUT
     *     - \ref AUXIOMODE_OPEN_SOURCE
     *     - \ref AUXIOMODE_OPEN_SOURCE_WITH_INPUT
     *     - \ref AUXIOMODE_ANALOG
     * \param[in]      pullLevel
     *     Pull level to be used when the pin is configured as input, open-drain or open-source
     *     - No pull: -1
     *     - Pull-down: 0
     *     - Pull-up: 1
     * \param[in]      outputValue
     *     Initial output value when the pin is configured as output, open-drain or open-source
     */
    pub(crate) unsafe fn scif_init_io(
        &self,
        aux_io_index: u32,
        io_mode: u32,
        pull_level: i32,
        output_value: u32,
    ) {
        // Calculate access parameters from the AUX I/O index
        if aux_io_index >= 8 {
            let (aux_aiodio, aux_aiodio_pin) = (&self.aux_aiodio1, aux_io_index - 8);

            // Setup the AUX I/O controller
            aux_aiodio
                .iomode
                .modify(|_r, w| w.bits(io_mode << (2 * aux_aiodio_pin)));
            aux_aiodio
                .gpiodout
                .modify(|_r, w| w.io7_0().bits((output_value as u8) << aux_aiodio_pin));
            aux_aiodio
                .gpiodie
                .modify(|_r, w| w.io7_0().bits(((io_mode >> 16) << aux_aiodio_pin) as u8));
            // Ensure that the settings have taken effect
            aux_aiodio.gpiodie.read();
        } else {
            let (aux_aiodio, aux_aiodio_pin) = (&self.aux_aiodio0, aux_io_index);

            // Setup the AUX I/O controller
            aux_aiodio
                .iomode
                .modify(|_r, w| w.bits(io_mode << (2 * aux_aiodio_pin)));
            aux_aiodio
                .gpiodout
                .modify(|_r, w| w.io7_0().bits((output_value as u8) << aux_aiodio_pin));
            aux_aiodio
                .gpiodie
                .modify(|_r, w| w.io7_0().bits(((io_mode >> 16) << aux_aiodio_pin) as u8));
            // Ensure that the settings have taken effect
            aux_aiodio.gpiodie.read();
        };

        // Configure pull level and transfer control of the I/O pin to AUX
        self.scif_reinit_io(aux_io_index, pull_level);
    } // scifInitIo

    /** \brief Re-initializes a single I/O pin for Sensor Controller usage
     *
     * This function must be called after the AUX AIODIO has been initialized, or when reinitializing I/Os
     * that have been lent temporarily to MCU domain peripherals. It only configures the following:
     * - IOC:
     *     -IOCFGn (index remapped using \ref scifData.pAuxIoIndexToMcuIocfgOffsetLut[])
     *
     * \param[in]      auxIoIndex
     *     Index of the I/O pin, 0-15, using AUX mapping
     * \param[in]      pullLevel
     *     Pull level to be used when the pin is configured as input, open-drain or open-source
     *     - No pull: -1
     *     - Pull-down: 0
     *     - Pull-up: 1
     */
    pub(crate) unsafe fn scif_reinit_io(&self, aux_io_index: u32, pull_level: i32) {
        // Calculate access parameters from the AUX I/O index
        let mcu_iocfg_offset: u32 =
            self.scif_data().aux_io_index_to_mcu_iocfg_offset_lut[aux_io_index as usize] as u32;

        // Setup the MCU I/O controller, making the AUX I/O setup effective
        let iocfg: u32 = driverlib::IOC_IOCFG0_PORT_ID_AUX_IO
            | match pull_level {
                -1 => driverlib::IOC_IOCFG0_PULL_CTL_DIS,
                0 => driverlib::IOC_IOCFG0_PULL_CTL_DWN,
                1 => driverlib::IOC_IOCFG0_PULL_CTL_UP,
                _ => unreachable!(), // FIXME: use enum instead of int
            };
        ((driverlib::IOC_BASE + driverlib::IOC_O_IOCFG0 + mcu_iocfg_offset) as *mut u32)
            .write_volatile(iocfg);
    } // scifReinitIo

    /** \brief Uninitializes a single I/O pin after Sensor Controller usage
     *
     * This detaches the I/O pin from the AUX domain, and configures it as GPIO with input/output disabled
     * and the specified pull level.
     *
     * \param[in]      auxIoIndex
     *     Index of the I/O pin, 0-15, using AUX mapping
     * \param[in]      pullLevel
     *     Pull level
     *     - No pull: -1
     *     - Pull-down: 0
     *     - Pull-up: 1
     */
    pub(crate) unsafe fn scif_uninit_io(&self, aux_io_index: u32, pull_level: i32) {
        // Calculate access parameters from the AUX I/O index
        let mcu_iocfg_offset: u32 =
            self.scif_data().aux_io_index_to_mcu_iocfg_offset_lut[aux_io_index as usize] as u32;

        // Unconfigure the MCU I/O controller (revert to GPIO with input/output disabled and desired pull
        // level)
        let iocfg: u32 = driverlib::IOC_IOCFG0_PORT_ID_AUX_IO
            | match pull_level {
                -1 => driverlib::IOC_IOCFG0_PULL_CTL_DIS,
                0 => driverlib::IOC_IOCFG0_PULL_CTL_DWN,
                1 => driverlib::IOC_IOCFG0_PULL_CTL_UP,
                _ => unreachable!(), // FIXME: use enum instead of int
            };
        ((driverlib::IOC_BASE + driverlib::IOC_O_IOCFG0 + mcu_iocfg_offset) as *mut u32)
            .write_volatile(iocfg);
    } // scifUninitIo

    unsafe fn scif_clear_ready_int_source(aux_evctl: &cc2650::AUX_EVCTL) {
        // Clear the source
        // HWREG(AUX_EVCTL_BASE + AUX_EVCTL_O_EVTOAONFLAGSCLR) = AUX_EVCTL_EVTOAONFLAGS_SWEV0_M;
        aux_evctl.evtoaonflagsclr.write(|w| w.swev0().set_bit());

        // Ensure that the source clearing has taken effect
        // while (HWREG(AUX_EVCTL_BASE + AUX_EVCTL_O_EVTOAONFLAGS) & AUX_EVCTL_EVTOAONFLAGS_SWEV0_M);
        while aux_evctl.evtoaonflags.read().swev0().bit_is_set() {}

        SCIF_READY.store(true, Ordering::Relaxed);
    } // scifClearAlertIntSource

    /** \brief Returns a bit-vector indicating the ALERT events associcated with the last ALERT interrupt
     *
     * This function shall be called by the application after it has received an ALERT interrupt, to find
     * which events have occurred.
     *
     * When all the alert events have been handled, the application must call \ref scifAckAlertEvents().
     * After acknowledging, this function must not be called again until the next ALERT event has been
     * received.
     *
     * \return
     *     The event bit-vector contains the following fields:
     *     - [15:8] Task input/output handling failed due to underflow/overflow, one bit per task ID
     *     - [7:0] Task input/output data exchange pending, one bit per task ID
     */
    unsafe fn scif_get_alert_events(&self) -> u32 {
        safe_packed_ref!(self.scif_data().task_ctrl.bv_task_io_alert).get() as u32
    } // scifGetAlertEvents

    /** \brief Clears the ALERT interrupt source
     *
     * The application must call this function once and only once after reception of an ALERT interrupt,
     * before calling \ref scifAckAlertEvents().
     */
    fn scif_clear_alert_int_source(aux_evctl: &cc2650::AUX_EVCTL) {
        // Clear the source
        aux_evctl.evtoaonflagsclr.write(|w| w.swev1().set_bit());

        // Ensure that the source clearing has taken effect
        while aux_evctl.evtoaonflags.read().swev1().bit_is_set() {}
    } // scifClearAlertIntSource

    /** \brief Acknowledges the ALERT events associcated with the last ALERT interrupt
     *
     * This function shall be called after the handling the events associated with the last ALERT interrupt.
     *
     * The application must call this function once and only once after reception of an ALERT event,
     * after calling \ref scifClearAlertIntSource(). It must not be called unless an ALERT event has
     * occurred.
     *
     * \note Calling this function can delay (by a short period of time) the next task to be executed.
     */
    unsafe fn scif_ack_alert_events(&self) {
        // Clear the events that have been handled now. This is needed for subsequent ALERT interrupts
        // generated by fwGenQuickAlertInterrupt(), since that procedure does not update bvTaskIoAlert.
        self.scif_data.map(|scif_data| {
            safe_packed_ref!(scif_data.task_ctrl.bv_task_io_alert).set(0x0000);
        });

        // Make sure that the CPU interrupt has been cleared before reenabling it
        Self::osal_clear_task_alert_int();
        let key = Self::scif_osal_enter_critical_section();
        Self::osal_enable_task_alert_int();

        // Set the ACK event to the Sensor Controller
        // FIXME: why + 1? why >> 8 ???
        // HWREGB(AUX_EVCTL_BASE + AUX_EVCTL_O_VECCFG1 + 1) = (AUX_EVCTL_VECCFG1_VEC3_EV_AON_SW | AUX_EVCTL_VECCFG1_VEC3_EN_M | AUX_EVCTL_VECCFG1_VEC3_POL_M) >> 8;
        // HWREGB(AUX_EVCTL_BASE + AUX_EVCTL_O_VECCFG1 + 1) = (AUX_EVCTL_VECCFG1_VEC3_EV_AON_SW | AUX_EVCTL_VECCFG1_VEC3_EN_M) >> 8;
        self.aux_evctl
            .veccfg1
            .write(|w| w.vec3_ev().aon_sw().vec3_en().en().vec3_pol().set_bit());
        self.aux_evctl
            .veccfg1
            .write(|w| w.vec3_ev().aon_sw().vec3_en().en());

        Self::scif_osal_leave_critical_section(key);
    } // scifAckAlertEvents

    /** \brief Sets the initial task startup delay, in ticks
     *
     * This function may be used when starting multiple tasks at once, allowing for either:
     * - Spreading the execution times, for reduced peak current consumption and precise execution timing
     * - Aligning the execution times, for reduced total current consumption but less precise timing for
     *   lower-priority tasks
     *
     * If used, note the following:
     * - It replaces the call to \c fwScheduleTask() from the "Initialization Code"
     * - This function must be used with care when timer-based tasks are already running
     * - This function must always be called when starting the relevant tasks
     *
     * \param[in]      taskId
     *     ID of the task to set startup delay for
     * \param[in]      ticks
     *     Number of timer ticks until the first execution
     */
    unsafe fn scif_set_task_startup_delay(&self, task_id: u32, ticks: u16) {
        self.scif_data()
            .task_execute_schedule
            .add(task_id as usize)
            .write_volatile(ticks);
    } // scifSetTaskStartupDelay

    /** \brief Resets the task data structures for the specified tasks
     *
     * This function must be called before tasks are restarted. The function resets the state data
     * structure, and optionally the \c input, \c output and \c state data structures.
     *
     * \param[in]      bvTaskIds
     *     Bit-vector indicating which tasks to reset (where bit N corresponds to task ID N)
     * \param[in]      bvTaskStructs
     *     Bit-vector indicating which task data structure types to reset in addition to \c state for these
     *     tasks. Make a bit-vector of \ref SCIF_STRUCT_CFG, \ref SCIF_STRUCT_INPUT and
     *     \ref SCIF_STRUCT_OUTPUT
     */
    pub(crate) unsafe fn scif_reset_task_structs(
        &self,
        mut bv_task_ids: u32,
        mut bv_task_structs: u32,
    ) {
        // Indicate that the data structure has been cleared
        self.scif_data
            .map(|scif_data| scif_data.bv_dirty_tasks &= !bv_task_ids as u16);

        // Always clean the state data structure
        bv_task_structs |= 1 << SCIFTaskStructType::SCIFStructState as u32;

        // As long as there are more tasks to reset ...
        while bv_task_ids != 0 {
            let task_id: u32 = bv_task_ids.trailing_zeros();

            bv_task_ids &= !(1 << task_id);

            // For each data structure to be reset ...
            while bv_task_structs != 0 {
                let n: u32 = bv_task_structs.trailing_zeros();
                bv_task_structs &= !(1 << n);

                let task_struct_info: u32 =
                    self.scif_data().task_data_struct_info_lut[(task_id * 4 + n) as usize];

                // If it exists ...
                if task_struct_info != 0 {
                    // Split the information
                    let mut addr: u16 = (task_struct_info >> 0) as u16 & 0x0FFF; // 11:0
                    let count: u16 = (task_struct_info >> 12) as u16 & 0x00FF; // 19:12
                    let size: u16 = (task_struct_info >> 20) as u16 & 0x0FFF; // 31:20
                    let mut length: u16 = core::mem::size_of::<u16>() as u16 * size * count;

                    // If multiple-buffered, include the control variables
                    if count > 1 {
                        addr -= SCIF_TASK_STRUCT_CTRL_SIZE as u16;
                        length += SCIF_TASK_STRUCT_CTRL_SIZE as u16;
                    }

                    // Reset the data structure
                    core::ptr::copy_nonoverlapping(
                        (self.scif_data().aux_ram_image.as_ptr() as *const u8).add(addr as usize),
                        (driverlib::AUX_RAM_BASE as *mut u8).add(addr as usize),
                        length as usize,
                    );
                }
            }
        }
    } // scifResetTaskStructs

    /** \brief Returns the number of input/output data buffers available for production/consumption
    *
    * When performing task data exchange with multiple-buffered data structures, the application calls this
    * function to get:
    * - The number of input buffers ready to be produced (\a taskStructType = \ref SCIF_STRUCT_INPUT)
    * - The number of output buffers ready to be consumed (\a taskStructType = \ref SCIF_STRUCT_OUTPUT)

    * The application is only allowed to access the returned number of buffers, or less. The function
    * always returns 0 if a buffer overrun or underrun has occurred but has not yet been reported by
    * \ref scifGetAlertEvents(). For each buffer to be produced or consumed, the application must complete
    * these steps:
    * - Call \ref scifGetTaskStruct() to get a pointer to the data structure
    * - If input, populate the data structure. If output, process the data structure contents and reset
    *   contents manually, as needed.
    * - Call \ref scifHandoffTaskStruct() to give/return the buffer to the Sensor Controller Engine
    *
    * For single-buffered data structures, the function has no effect and always returns 0.
    *
    * \param[in]      taskId
    *     Task ID selection
    * \param[in]      taskStructType
    *     Task data structure type selection
    *
    * \return
    *     The number of buffers that can be produced/consumed by the application
    */
    unsafe fn scif_get_task_io_struct_avail_count(
        &self,
        task_id: u32,
        task_struct_type: SCIFTaskStructType,
    ) -> u32 {
        // Fetch the information about the data structure
        let task_struct_info: u32 = self.scif_data().task_data_struct_info_lut
            [(task_id * 4 + task_struct_type as u32) as usize];
        let base_addr: u16 = (task_struct_info >> 0) as u16 & 0x0FFF; // 11:0
        let count: u16 = (task_struct_info >> 12) as u16 & 0x00FF; // 19:12
        let size: u16 = (task_struct_info >> 20) as u16 & 0x0FFF; // 31:20

        // If single-buffered, it's always 0
        if count < 2 {
            return 0;
        }

        // Fetch the current memory addresses used by SCE and MCU
        let mut sce_addr: u16 = ((driverlib::AUX_RAM_BASE + base_addr as u32
            - SCIF_TASK_STRUCT_CTRL_SCE_ADDR_BACK_OFFSET)
            as *const u16)
            .read_volatile();
        let mut mcu_addr: u16 = ((driverlib::AUX_RAM_BASE + base_addr as u32
            - SCIF_TASK_STRUCT_CTRL_MCU_ADDR_BACK_OFFSET)
            as *const u16)
            .read_volatile();

        // Buffer overflow or underflow can occur in the background if the Sensor Controller produces or
        // consumes data too fast for the System CPU application. If this happens, return 0 so that the
        // application can detect the error by calling scifGetAlertEvents() in the next ALERT interrupt
        // before starting to process potentially corrupted or out-of-sync buffers.
        if safe_packed_ref!(self.scif_data().int_data.bv_task_io_alert).get() & (0x0100 << task_id)
            != 0
        {
            return 0;
        }

        // Detect all buffers available
        // LSBs are different when none are available -> handled in the calculation further down
        if mcu_addr == sce_addr {
            return count as u32;
        }

        // Calculate the number of buffers available
        mcu_addr &= !0x0001;
        sce_addr &= !0x0001;
        if sce_addr < mcu_addr {
            sce_addr += size * core::mem::size_of::<u16>() as u16 * count;
        }

        ((sce_addr - mcu_addr) / (size * core::mem::size_of::<u16>() as u16)) as u32
    } // scifGetTaskIoStructAvailCount

    /** \brief Returns a pointer to the specified data structure
     *
     * This function must be used to access multiple-buffered data structures, in which case it finds the
     * correct data structure buffer to be produced/consumed by the application. The application must use
     * \ref scifGetTaskIoStructAvailCount() to get the number of buffers to produce/consume.
     *
     * This function can also be used for single-buffered data structures, but this is less efficient than
     * accessing these data structures directly.
     *
     * \param[in]      taskId
     *     Task ID selection
     * \param[in]      taskStructType
     *     Task data structure type selection
     *
     * \return
     *     Pointer to the data structure (must be casted to correct pointer type)
     */
    unsafe fn scif_get_task_struct(
        &self,
        task_id: u32,
        task_struct_type: SCIFTaskStructType,
    ) -> *mut () {
        // Fetch the information about the data structure
        let task_struct_info: u32 = self.scif_data().task_data_struct_info_lut
            [(task_id * 4 + task_struct_type as u32) as usize];
        let base_addr: u16 = (task_struct_info >> 0) as u16 & 0x0FFF; // 11:0
        let count: u16 = (task_struct_info >> 12) as u16 & 0x00FF; // 19:12

        // If single-buffered, just return the base address
        if count < 2 {
            (driverlib::AUX_RAM_BASE as *mut ()).add(base_addr as usize)

        // If multiple-buffered, return the MCU address
        } else {
            let mcu_addr: u16 = (driverlib::AUX_RAM_BASE as *const u16)
                .add(base_addr as usize)
                .sub(SCIF_TASK_STRUCT_CTRL_MCU_ADDR_BACK_OFFSET as usize)
                .read_volatile();
            (driverlib::AUX_RAM_BASE as *mut ()).add(mcu_addr as usize & !0x0001)
        }
    } // scifGetTaskStruct

    /** \brief Called to handoff the an input or output data structure to the Sensor Controller Engine
     *
     * For output, this function shall be called after consuming a buffer of a multiple-buffered data
     * structure. This hands over the output buffer back to the Sensor Controller.
     *
     * For input, this function shall be called after producing a buffer of a multiple-buffered data
     * structure. This hands over the input buffer to the Sensor Controller.
     *
     * \param[in]      taskId
     *     Task ID selection
     * \param[in]      taskStructType
     *     Task data structure type selection
     */
    unsafe fn scif_handoff_task_struct(&self, task_id: u32, task_struct_type: SCIFTaskStructType) {
        // Fetch the information about the data structure
        let task_struct_info: u32 = self.scif_data().task_data_struct_info_lut
            [(task_id * 4 + task_struct_type as u32) as usize];
        let base_addr: u16 = (task_struct_info >> 0) as u16 & 0x0FFF; // 11:0
        let count: u16 = (task_struct_info >> 12) as u16 & 0x00FF; // 19:12
        let size: u16 = (task_struct_info >> 20) as u16 & 0x0FFF; // 31:20

        // If multiple-buffered, move on the MCU address to the next buffer
        if count >= 2 {
            // Move on the address
            let p_mcu_addr: *mut u16 = (driverlib::AUX_RAM_BASE + base_addr as u32
                - SCIF_TASK_STRUCT_CTRL_MCU_ADDR_BACK_OFFSET)
                as *mut u16;
            let mut new_mcu_addr: u16 = *p_mcu_addr + size * core::mem::size_of::<u16>() as u16;

            // If it has wrapped, move it back to the start and invert LSB
            if new_mcu_addr & !0x0001
                > (base_addr + (size * core::mem::size_of::<u16>() as u16 * (count - 1)))
            {
                new_mcu_addr = base_addr | ((new_mcu_addr & 0x0001) ^ 0x0001);
            }

            // Write back the new address
            p_mcu_addr.write_volatile(new_mcu_addr);
        }
    } // scifHandoffTaskStruct

    /** \brief Common function for manually starting, executing and terminating tasks
     *
     * \param[in]      bvTaskIds
     *     Bit-vector of task IDs for the tasks to be controlled
     * \param[in]      bvTaskReq
     *     Any legal combination of the following bits:
     *     - 0x01 : Starts the specified tasks
     *     - 0x02 : Executes the specified tasks once
     *     - 0x04 : Stops the specified tasks
     *
     * \return
     *     \ref SCIF_SUCCESS if successful, otherwise \ref SCIF_NOT_READY (last non-blocking call has not
     *     completed) or SCIF_ILLEGAL_OPERATION (attempted to execute an already active task). The function
     *     call has no effect if unsuccessful.
     */
    unsafe fn scif_ctrl_tasks_nbl(&self, bv_task_ids: u32, bv_task_req: u32) -> SCIFResult {
        // Prevent interruptions by concurrent scifCtrlTasksNbl() calls
        if !Self::osal_lock_ctrl_task_nbl() {
            return SCIFResult::NotReady;
        }

        // Perform sanity checks: Starting already active or dirty tasks is illegal
        if bv_task_req & 0x01 != 0 {
            let task_ctrl = self.scif_data().task_ctrl;
            if (safe_packed_ref!(task_ctrl.bv_active_tasks).get() | self.scif_data().bv_dirty_tasks)
                & (bv_task_ids as u16)
                != 0
            {
                Self::osal_unlock_ctrl_task_nbl();
                return SCIFResult::IllegalOperation;
            }
        }

        // Verify that the control interface is ready
        if !SCIF_READY.swap(false, Ordering::Relaxed) {
            Self::osal_unlock_ctrl_task_nbl();
            return SCIFResult::NotReady;
        }

        self.scif_data.map(|scif_data| {
            let task_ctl_data = scif_data.task_ctrl;

            // Initialize tasks?
            safe_packed_ref!(task_ctl_data.bv_task_initialize_req).set(
                if bv_task_req & 0x01 != 0 {
                    scif_data.bv_dirty_tasks |= bv_task_ids as u16;
                    bv_task_ids as u16
                } else {
                    0x0000
                },
            );

            // Execute tasks?
            safe_packed_ref!(task_ctl_data.bv_task_execute_req).set(if bv_task_req & 0x02 != 0 {
                bv_task_ids as u16
            } else {
                0x0000
            });

            // Terminate tasks? Terminating already inactive tasks is allowed, because tasks may stop
            // spontaneously, and there's no way to know this for sure (it may for instance happen at any moment
            // while calling this function)
            safe_packed_ref!(task_ctl_data.bv_task_terminate_req).set(
                if (bv_task_req & 0x04) != 0 {
                    bv_task_ids as u16
                } else {
                    0x0000
                },
            );
        });

        // Make sure that the CPU interrupt has been cleared before reenabling it
        Self::osal_clear_ctrl_ready_int();
        Self::osal_enable_ctrl_ready_int();

        // Set the REQ event to hand over the request to the Sensor Controller

        self.aux_evctl
            .veccfg0
            .modify(|_r, w| w.vec0_pol().set_bit());
        self.aux_evctl
            .veccfg0
            .modify(|_r, w| w.vec0_pol().clear_bit());
        Self::osal_unlock_ctrl_task_nbl();

        SCIFResult::Success
    } // scifCtrlTasksNbl

    /** \brief Executes the specified tasks once
     *
     * This triggers the initialization, execution and termination code for each task ID specified in
     * \a bvTaskIds. All selected code is completed for one task before proceeding with the next task. The
     * control READY event is generated when the tasks have been executed.
     *
     * This function should not be used to execute a task that implements the event handler code, because
     * the execution method does not allow for running the event handler code.
     *
     * This function must not be called for already active tasks.
     *
     * \note Calling this function can delay the next (previously active) task to be executed, if any.
     *
     * \param[in]      bvTaskIds
     *     Bit-vector indicating which tasks should be executed (where bit N corresponds to task ID N)
     *
     * \return
     *     \ref SCIF_SUCCESS if successful, otherwise \ref SCIF_NOT_READY (last non-blocking call has not
     *     completed) or \ref SCIF_ILLEGAL_OPERATION (attempted to execute an already active task). The
     *     function call has no effect if unsuccessful.
     */
    pub(crate) unsafe fn scif_execute_tasks_once_nbl(&self, bv_task_ids: u16) -> SCIFResult {
        self.scif_ctrl_tasks_nbl(bv_task_ids as u32, 0x07)
    } // scifExecuteTasksOnceNbl

    /** \brief Starts the specified tasks
     *
     * This triggers the initialization code for each task ID specified in \a bvTaskIds. The READY event
     * is generated when the tasks have been started.
     *
     * \note This function must not be called for already active tasks.
     *
     * \note Calling this function can delay the next (previously active) task to be executed, if any.
     *
     * \param[in]      bvTaskIds
     *     Bit-vector indicating which tasks to be started (where bit N corresponds to task ID N)
     *
     * \return
     *     \ref SCIF_SUCCESS if successful, otherwise \ref SCIF_NOT_READY (last non-blocking call has not
     *     completed) or \ref SCIF_ILLEGAL_OPERATION (attempted to start an already active task). The
     *     function call has no effect if unsuccessful.
     */
    unsafe fn scif_start_tasks_nbl(&self, bv_task_ids: u16) -> SCIFResult {
        self.scif_ctrl_tasks_nbl(bv_task_ids as u32, 0x01)
    } // scifStartTasksNbl

    /** \brief Stops the specified tasks
     *
     * This triggers the termination code for each task ID specified in \a bvTaskIds. The READY event is
     * generated when the tasks have been stopped.
     *
     * \note Calling this function can delay the next (still active) task to be executed, if any.
     *
     * \param[in]      bvTaskIds
     *     Bit-vector indicating which tasks to be stopped (where bit N corresponds to task ID N)
     *
     * \return
     *     \ref SCIF_SUCCESS if successful, otherwise \ref SCIF_NOT_READY (last non-blocking call has not
     *     completed). The function call has no effect if unsuccessful.
     */
    unsafe fn scif_stop_tasks_nbl(&self, bv_task_ids: u16) -> SCIFResult {
        self.scif_ctrl_tasks_nbl(bv_task_ids as u32, 0x04)
    } // scifStopTasksNbl

    /** \brief Waits for a non-blocking call to complete, with timeout
     *
     * The non-blocking task control functions, \ref scifExecuteTasksOnceNbl(), \ref scifStartTasksNbl()
     * and \ref scifStopTasksNbl(), may take some time to complete. This wait function can be used to make
     * blocking calls (i.e. where the OS switches thread when not ready).
     *
     * The function returns when the last non-blocking call has completed, or immediately if already
     * completed.
     *
     * \b Important: Unlike the ALERT event, the READY event does not generate MCU domain and System CPU
     * wake-up. Depending on the SCIF OSAL implementation, this function might not return before the
     * specified timeout expires, even if the READY event has occurred long before that. To a such
     * delays, call \c fwGenAlertInterrupt() from the task code block that \ref scifWaitOnNbl() is waiting
     * for to complete.
     *
     * \param[in]      timeoutUs
     *     Maximum number of microseconds to wait for the non-blocking functions to become available. Use a
     *     timeout of "0" to check whether the interface already is available, or simply call the control
     *     function (which also will return \ref SCIF_NOT_READY if not ready).
     *
     * \return
     *     \ref SCIF_SUCCESS if the last call has completed, otherwise \ref SCIF_NOT_READY.
     */
    unsafe fn scif_wait_on_nbl(&self, timeout_us: u32) -> SCIFResult {
        // if (HWREG(driverlib::AUX_EVCTL_BASE + AUX_EVCTL_O_EVTOAONFLAGS) & AUX_EVCTL_EVTOAONFLAGS_SWEV0_M) || osalWaitOnCtrlReady(timeout_us) {
        if self.aux_evctl.evtoaonflags.read().swev0().bit_is_set()
            || self.osal_wait_on_ctrl_ready(timeout_us)
        {
            SCIFResult::Success
        } else {
            SCIFResult::NotReady
        }
    } // scifWaitOnNbl

    /** \brief Returns a bit-vector indicating which tasks are currently active
     *
     * The bit-vector is maintained by the Sensor Controller. If called before the last non-blocking control
     * operation has completed, the bit-vector may indicate the task states as before or after the non-
     * blocking operations (the bit vector is updated only once per non-blocking call).
     *
     * \return
     *     A bit-vector indicating which tasks are active (bit N corresponds to task N)
     */
    unsafe fn scif_get_active_task_ids(&self) -> u16 {
        safe_packed_ref!(self.scif_data().task_ctrl.bv_active_tasks).get()
    } // scifGetActiveTaskIds
}

/*
 * whip6: Warsaw High-performance IPv6.
 *
 * Copyright (c) 2012-2017 Szymon Acedanski
 * All rights reserved.
 *
 * This file is distributed under the terms in the attached LICENSE
 * files.
 */
/** \addtogroup module_scif_generic_interface Generic Driver Interface
 *
 * \section section_usage Usage
 * A generic driver API and framework is used to control and exchange data with the Sensor Controller.
 * See below for guidelines on:
 * - \ref section_scif_init
 * - \ref section_scif_uninit
 * - \ref section_scif_aux_domain_access
 * - \ref section_scif_task_struct_access
 * - \ref section_scif_task_control
 * - \ref section_scif_usage_data_exchange
 *
 *
 * \subsection section_scif_init Initialization
 * The driver must be initialized before use by calling \ref scifInit(), with a pointer to the
 * \ref module_scif_driver_setup to be used, \ref self.scif_data_T. The call:
 * - Verifies that the driver is not already active
 * - Stores a local copy of the driver setup data structure, \ref self.scif_data_T
 * - Configures AUX domain hardware modules for operation (general and setup-specific). This includes
 *   complete I/O setup for all Sensor Controller tasks.
 * - Loads the generated AUX RAM image into the AUX RAM
 * - Initializes handshaking mechanisms for control, alert interrupt generation and data exchange
 * - Configures use of AUX domain wake-up sources
 * - Starts the Sensor Controller
 *
 * Additional initialization may be required depending on the selected \ref module_scif_osal and enabled
 * Sensor Controller task resources. For example, when using RTC-based task scheduling and TI-RTOS, the
 * following calls must be made:
 * \code
 * scifOsalInit();
 * scifOsalRegisterCtrlReadyCallback( ... );
 * scifOsalRegisterTaskAlertCallback( ... );
 * scifInit(&scifDriverSetup);
 * scifStartRtcTicksNow( ... );
 * \endcode
 *
 * If using RTC-based task scheduling, the application or OS is also required to start the RTC itself.
 * This can be done at any point, independently of calls to \ref scifInit().
 *
 *
 * \subsection section_scif_uninit Uninitialization and Driver Switching
 * It is possible to have multiple Sensor Controller Interface drivers in one application, however these
 * are not allowed to run at the same time. To switch from one driver to another, the currently used
 * driver must be uninitialized first by calling \ref scifUninit().
 * \code
 * // Load the driver setup with prefix "ABC"
 * scifInit(&scifAbcData);
 *
 * ...
 *
 * // Switch to the driver setup with prefix "XYZ"
 * scifUninit();
 * scifInit(&scifXyzData);
 *
 * ...
 * \endcode
 *
 * Additional uninitialization may be required depending on the selected \ref module_scif_osal and
 * enabled Sensor Controller task resources.
 *
 *
 * \subsection section_scif_aux_domain_access AUX Domain Access
 * If the Sensor Controller is used (i.e. \ref scifInit() is called), the AUX domain will by default not
 * have any clock while the Sensor Controller is idle. In this state, the System CPU and DMA will not
 * have access to the following AUX domain and SCIF resources:
 * - Oscillator control registers
 * - AUX ADI registers for ADC, analog comparators, current source etc.
 * - AUX sub-module registers and AUX RAM
 * - SCIF API functions
 * - SCIF data structures located in AUX RAM
 *
 * The implementation of AUX domain access control is OS-specific. For some OSAL implementations
 * (e.g. "TI-RTOS"), this is handled automatically by the OS framework so that the features listed above
 * are in practice always accessible to the application. For other OSAL implementations (e.g. "None"),
 * the application must call provided AUX domain access control functions whenever it needs or no longer
 * needs access to the AUX domain and SCIF features.
 *
 *
 * \subsection section_scif_task_struct_access Task Data Structure Access
 * The task data structures can be accessed in two ways:
 * - Directly through the task data structure instance (located in AUX RAM)
 * - Indirectly using \ref scifGetTaskStruct(), with task ID and data structure type as input
 *
 * The first method is more efficient, and be used with the \c cfg and \c state structures, and
 * single-buffered \c input and \c output data structures.
 *
 * The second method can be used for any data structure, including multiple-buffered \c input and
 * \c output data structures, where it will select the correct buffer (to be accessed by the
 * application) automatically.
 *
 *
 * \subsection section_scif_task_control Task Control
 * Sensor Controller tasks are started, stopped or can be executed once using these generic functions:
 * - scifStartTasksNbl() - Starts the specified tasks, by triggering their Initialization code
 *     - If needed, \ref scifSetTaskStartupDelay() can be used to skew the execution timing for the
 *       tasks started. This may also be used to spread or concentrate in time data exchange processing.
 * - scifStopTasksNbl() - Stops the specified tasks, by cancelling any scheduled execution and running
 *   the Termination code.
 * - scifExecuteTasksOnceNbl() - Triggers the Initialization, Execution and Termination code once for
 *   each specified task.
 *
 * The above functions are non-blocking in the sense that they do not wait for the Sensor Controller to
 * run the task code specified. To wait for the last non-blocking call to finish, or check if it has,
 * the application can call \ref scifWaitOnNbl(), or wait for the task control READY interrupt
 * (depending on the OSAL implementation).
 *
 * To check which tasks are currently active, use \ref scifGetActiveTaskIds().
 *
 * Before starting a task, the application may be required to set parameters in the task's configuration
 * data structure. For example:
 * \code
 * scifTaskData.lightSensor.cfg.lowThreshold  = 42;
 * scifTaskData.lightSensor.cfg.highThreshold = 123;
 * scifStartTasksNbl(BV(SCIF_LIGHT_SENSOR_TASK_ID));
 * \endcode
 *
 * When restarting a task, the associated data structures will be modified, and the task data structures
 * will be marked as "dirty". To reset data structures and remove the "dirty" condition, call
 * \ref scifResetTaskStructs() to reset the data structure contents as needed (by copying the original
 * structure contents). Resetting the \c state data structure is mandatory.
 *
 *
 * \subsection section_scif_usage_data_exchange Data Exchange
 * Task data exchange is performed on the Sensor Controller's initiative when the task code calls:
 * - \c fwGenAlertInterrupt() or \c fwGenQuickAlertInterrupt() for single-buffered data exchange or
 *   other proprietary use
 * - \c fwSwitchOutputBuffer() for multiple-buffered output data exchange
 *
 * Calling either of these procedures triggers a task ALERT interrupt from the driver, which, depending
 * on the OSAL implementation, allows the application to consume/process generated output data or
 * produce/set new input data.
 *
 * When processing the ALERT interrupt, the application must:
 * - Call \ref scifClearAlertIntSource() to clear the interrupt source from AUX_EVCTL
 * - Call \ref scifGetAlertEvents() to determine which tasks have pending events. If there is only one
 *   task and there is no risk of buffer overflow or underflow, this can be skipped. For each task with
 *   pending events:
 *     - For single-buffered input/output data exchange:
 *         - Access the task's output data, either directly or indirectly using \ref scifGetTaskStruct()
 *     - For multiple-buffered output data exchange:
 *         - Call scifGetTaskIoStructAvailCount() to get the number of buffers to be exchanged, and
 *           repeat the following steps the indicated number of times:
 *             - Call \ref scifGetTaskStruct() to get a pointer to the correct buffer
 *             - Access the data structure
 *             - Call \ref scifHandoffTaskStruct() to hand over the buffer to the Sensor Controller
 * - Call \ref scifAckAlertEvents() to acknowledge the handled events. If additional events have
 *   occurred in the meantime, the ALERT interrupt will be triggered again.
 *
 * @{
 */

/// Sensor Controller Interface function call result
#[derive(Debug)]
pub(crate) enum SCIFResult {
    /// Call succeeded
    Success = 0,
    /// Not ready (previous non-blocking call is still running)
    NotReady = 1,
    /// Illegal operation
    IllegalOperation = 2,
}

impl SCIFResult {
    #[track_caller]
    pub(crate) fn unwrap(self) {
        match self {
            SCIFResult::Success => (),
            SCIFResult::NotReady | SCIFResult::IllegalOperation => panic!("Unwrapped {:?}", self),
        }
    }
}

/// Task data structure types
#[repr(u32)]
pub(crate) enum SCIFTaskStructType {
    /// Task configuration data structure (Sensor Controller read-only)
    SCIFStructCfg = 0,
    /// Task input data structure
    SCIFStructInput = 1,
    /// Task output data structure
    SCIFStructOutput = 2,
    /// Task state data structure
    SCIFStructState = 3,
}

///  function pointer type: " func(Scif)"
type SCIFVfptr = unsafe fn(&Scif);

/// Sensor Controller internal data (located in AUX RAM)
#[repr(packed)]
pub(crate) struct SCIFIntData {
    /// ID of currently executed Sensor Controller task
    task_id: VolatileCell<u16>,
    /// Pending input/output data alert (LSB = normal exchange, MSB = overflow or underflow)
    bv_task_io_alert: VolatileCell<u16>,
    /// ALERT interrupt generation mask
    alert_gen_mask: VolatileCell<u16>,
}

/// Sensor Controller generic task control (located in AUX RAM)
#[repr(packed)]
pub(crate) struct SCIFTaskCtrl {
    /// Indicates which tasks are currently active (only valid while ready)
    bv_active_tasks: VolatileCell<u16>,
    /// Input/output data alert (LSB = normal exchange, MSB = overflow or underflow)
    bv_task_io_alert: VolatileCell<u16>,
    /// Requests tasks to start
    bv_task_initialize_req: VolatileCell<u16>,
    /// Requests tasks to execute once immediately
    bv_task_execute_req: VolatileCell<u16>,
    /// Requests tasks to stop
    bv_task_terminate_req: VolatileCell<u16>,
}

/// Driver internal data (located in main RAM, not shared with the Sensor Controller)
#[derive(Clone, Copy)]
pub(crate) struct SCIFData {
    /// Sensor Controller internal data (located in AUX RAM)
    pub(crate) int_data: &'static SCIFIntData,
    /// Sensor Controller task generic control (located in AUX RAM)
    pub(crate) task_ctrl: &'static SCIFTaskCtrl,
    /// Pointer to the task execution scheduling table
    pub(crate) task_execute_schedule: *mut u16,
    /// Bit-vector indicating tasks with potentially modified input/output/state data structures
    pub(crate) bv_dirty_tasks: u16,

    /// AUX RAM image word array
    pub(crate) aux_ram_image: &'static [u16],

    /// Look-up table that converts from AUX I/O index to MCU IOCFG offset
    pub(crate) task_data_struct_info_lut: &'static [u32],
    /// Look-up table of data structure information for each task
    pub(crate) aux_io_index_to_mcu_iocfg_offset_lut: &'static [u8],

    /// Pointer to the project-specific hardware initialization function
    pub(crate) fptr_task_resource_init: SCIFVfptr,
    /// Pointer to the project-specific hardware uninitialization function
    pub(crate) fptr_task_resource_uninit: SCIFVfptr,
}

/// I/O pin mode: Output
pub(crate) const AUXIOMODE_OUTPUT: u32 = 0x00000000;
/// I/O pin mode: Input, active
pub(crate) const AUXIOMODE_INPUT: u32 = 0x00010001;
/// I/O pin mode: Input, inactive
pub(crate) const AUXIOMODE_INPUT_IDLE: u32 = 0x00000001;
/// I/O pin mode: Open drain (driven low, pulled high)
pub(crate) const AUXIOMODE_OPEN_DRAIN: u32 = 0x00000002;
/// I/O pin mode: Open drain + input (driven low, pulled high, input buffer enabled)
pub(crate) const AUXIOMODE_OPEN_DRAIN_WITH_INPUT: u32 = 0x00010002;
/// I/O pin mode: Open source (driven high, pulled low)
pub(crate) const AUXIOMODE_OPEN_SOURCE: u32 = 0x00000003;
/// I/O pin mode: Open source + input (driven high, pulled low, input buffer enabled)
pub(crate) const AUXIOMODE_OPEN_SOURCE_WITH_INPUT: u32 = 0x00010003;
/// I/O pin mode: Analog
pub(crate) const AUXIOMODE_ANALOG: u32 = 0x00000001;

/*

// Driver main control
SCIF_RESULT_T scifInit(const self.scif_data_T* pScifDriverSetup);
 scifUninit();

 scifClearReadyIntSource();

// Driver ALERT interrupt handling
u32 scifGetAlertEvents();
 scifClearAlertIntSource();
 scifAckAlertEvents();

// Task generic configuration functions
 scifSetTaskStartupDelay(u32 taskId, u16 ticks);

// Task data structure access functions
 scifResetTaskStructs(u32 bvTaskIds, u32 bvTaskStructs);
u32 scifGetTaskIoStructAvailCount(u32 taskId, SCIF_TASK_STRUCT_TYPE_T taskStructType);
* scifGetTaskStruct(u32 taskId, SCIF_TASK_STRUCT_TYPE_T taskStructType);
 scifHandoffTaskStruct(u32 taskId, SCIF_TASK_STRUCT_TYPE_T taskStructType);

// Task control functions (non-blocking)
SCIF_RESULT_T scifExecuteTasksOnceNbl(u16 bvTaskIds);
SCIF_RESULT_T scifStartTasksNbl(u16 bvTaskIds);
SCIF_RESULT_T scifStopTasksNbl(u16 bvTaskIds);
SCIF_RESULT_T scifWaitOnNbl(u32 timeoutUs);

// Task status functions
u16 scifGetActiveTaskIds();
*/

// ####### END scif_framework.h

// ####### scif_framework.c

/*
* whip6: Warsaw High-performance IPv6.
*
* Copyright (c) 2012-2017 Szymon Acedanski
* All rights reserved.
*
* This file is distributed under the terms in the attached LICENSE
* files.
*/

/// \addtogroup module_scif_generic_interface

/// Task data structure buffer control: Size (in bytes)
const SCIF_TASK_STRUCT_CTRL_SIZE: u32 = 3 * core::mem::size_of::<u16>() as u32;
/// Task data structure buffer control: Sensor Controller Engine's pointer negative offset (ref. struct start)
const SCIF_TASK_STRUCT_CTRL_SCE_ADDR_BACK_OFFSET: u32 = 3 * core::mem::size_of::<u16>() as u32;
/// Task data structure buffer control: Driver/MCU's pointer negative offset (ref. struct start)
const SCIF_TASK_STRUCT_CTRL_MCU_ADDR_BACK_OFFSET: u32 = 2 * core::mem::size_of::<u16>() as u32;

// ###### END scif_framework.c

// ###### BEGIN scif_osal.c

/// MCU wakeup source to be used with the Sensor Controller task ALERT event, must not conflict with OS
const OSAL_MCUWUSEL_WU_EV_S: u32 = driverlib::AON_EVENT_MCUWUSEL_WU3_EV_S;

/// The READY interrupt is implemented using INT_AON_AUX_SWEV0
const INT_SCIF_CTRL_READY: u32 = driverlib::INT_AON_AUX_SWEV0;
/// The ALERT interrupt is implemented using INT_AON_AUX_SWEV1
const INT_SCIF_TASK_ALERT: u32 = driverlib::INT_AON_AUX_SWEV1;

/// Calculates the NVIC register offset for the specified interrupt
fn NVIC_OFFSET(i: u32) -> u32 {
    (i - 16) / 32
}
/// Calculates the bit-vector to be written or compared against for the specified interrupt
fn NVIC_BV(i: u32) -> u32 {
    1 << ((i - 16) % 32)
}

impl Scif {
    /** \brief Enters a critical section by disabling hardware interrupts
     *
     * \return
     *     Whether interrupts were enabled at the time this function was called
     */
    unsafe fn scif_osal_enter_critical_section() -> bool {
        return driverlib::CPUcpsid() == 0;
    } // scifOsalEnterCriticalSection

    /** \brief Leaves a critical section by reenabling hardware interrupts if previously enabled
     *
     * \param[in]      key
     *     The value returned by the previous corresponding call to \ref scifOsalEnterCriticalSection()
     */
    unsafe fn scif_osal_leave_critical_section(key: bool) {
        if key {
            driverlib::CPUcpsie();
        }
    } // scifOsalLeaveCriticalSection

    /// Stores whether task control non-blocking functions have been locked
    //static volatile bool osalCtrlTaskNblLocked = false;

    /** \brief Locks use of task control non-blocking functions
     *
     * This function is used by the non-blocking task control to allow safe operation from multiple threads.
     *
     * The function shall attempt to set the \ref osalCtrlTaskNblLocked flag in a critical section.
     * Implementing a timeout is optional (the task control's non-blocking behavior is not associated with
     * this critical section, but rather with completion of the task control request).
     *
     * \return
     *     Whether the critical section could be entered (true if entered, false otherwise)
     */
    fn osal_lock_ctrl_task_nbl() -> bool {
        /*uint32_t key = !CPUcpsid();
        if (osalCtrlTaskNblLocked) {
            if (key) CPUcpsie();
            return false;
        } else {
            osalCtrlTaskNblLocked = true;
            if (key) CPUcpsie();
            return true;
        }*/
        return true;
    } // osalLockCtrlTaskNbl

    /** \brief Unlocks use of task control non-blocking functions
     *
     * This function will be called once after a successful \ref osalLockCtrlTaskNbl().
     */
    fn osal_unlock_ctrl_task_nbl() {
        //osalCtrlTaskNblLocked = false;
    } // osalUnlockCtrlTaskNbl

    pub(crate) unsafe extern "C" fn ready_handler() {
        let aux_evctl = cc2650::Peripherals::steal().AUX_EVCTL;
        Self::scif_clear_ready_int_source(&aux_evctl);
        // HWREG(driverlib::NVIC_DIS0 + NVIC_OFFSET(INT_SCIF_CTRL_READY)) = NVIC_BV(INT_SCIF_CTRL_READY);
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_CTRL_READY);
        n.disable();
    }

    pub(crate) unsafe extern "C" fn alert_handler() {
        let aux_evctl = cc2650::Peripherals::steal().AUX_EVCTL;
        Self::scif_clear_alert_int_source(&aux_evctl);
        // HWREG(driverlib::NVIC_DIS0 + NVIC_OFFSET(INT_SCIF_TASK_ALERT)) = NVIC_BV(INT_SCIF_TASK_ALERT);
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_TASK_ALERT);
        n.disable();
    }

    /** \brief Enables the control READY interrupt
     *
     * This function is called when sending a control REQ event to the Sensor Controller to enable the READY
     * interrupt. This is done after clearing the event source and then the READY interrupt, using
     * \ref osalClearCtrlReadyInt().
     */
    unsafe fn osal_enable_ctrl_ready_int() {
        // FIXME: interrupt.c brings GBs to the bin file...
        // driverlib::IntRegister(driverlib::INT_AUX_SWEV0, Some(Self::ready_handler));
        // HWREG(NVIC_EN0 + NVIC_OFFSET(INT_SCIF_CTRL_READY)) = NVIC_BV(INT_SCIF_CTRL_READY);
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_CTRL_READY);
        n.enable();
    } // osalEnableCtrlReadyInt

    unsafe fn osal_disable_ctrl_ready_int() {
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_CTRL_READY);
        n.disable();
        // FIXME: interrupt.c brings GBs to the bin file...
        // driverlib::IntUnregister(driverlib::INT_AUX_SWEV0);
    }

    /** \brief Clears the task control READY interrupt
     *
     * This is done when sending a control request, after clearing the READY source event.
     */
    unsafe fn osal_clear_ctrl_ready_int() {
        // HWREG(NVIC_UNPEND0 + NVIC_OFFSET(INT_SCIF_CTRL_READY)) = NVIC_BV(INT_SCIF_CTRL_READY);
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_CTRL_READY);
        n.clear_pending();
    } // osalClearCtrlReadyInt

    /** \brief Enables the task ALERT interrupt
     *
     * The interrupt is enabled at startup. It is disabled upon reception of a task ALERT interrupt and re-
     * enabled when the task ALERT is acknowledged.
     */
    unsafe fn scif_osal_enable_task_alert_int() {
        // HWREG(NVIC_EN0 + NVIC_OFFSET(INT_SCIF_TASK_ALERT)) = NVIC_BV(INT_SCIF_TASK_ALERT);
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_TASK_ALERT);
        n.enable();
        // FIXME: interrupt.c brings GBs to the bin file...
        // driverlib::IntRegister(driverlib::INT_AUX_SWEV1, Some(Self::alert_handler));
    } // scifOsalEnableTaskAlertInt

    /** \brief Clears the task ALERT interrupt
     *
     * This is done when acknowledging the alert, after clearing the ALERT source event.
     */
    unsafe fn osal_clear_task_alert_int() {
        // HWREG(NVIC_UNPEND0 + NVIC_OFFSET(INT_SCIF_TASK_ALERT)) = NVIC_BV(INT_SCIF_TASK_ALERT);
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_TASK_ALERT);
        n.clear_pending();
    } // osalClearTaskAlertInt

    unsafe fn osal_enable_task_alert_int() {
        // HWREG(NVIC_EN0 + NVIC_OFFSET(INT_SCIF_TASK_ALERT)) = NVIC_BV(INT_SCIF_TASK_ALERT);
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_TASK_ALERT);
        n.enable();
        // FIXME: interrupt.c brings GBs to the bin file...
        // driverlib::IntRegister(driverlib::INT_AUX_SWEV1, Some(Self::alert_handler));
    } // scifOsalEnableTaskAlertInt

    unsafe fn osal_disable_task_alert_int() {
        let n = cortexm3::nvic::Nvic::new(INT_SCIF_TASK_ALERT);
        n.disable();
        // FIXME: interrupt.c brings GBs to the bin file...
        // driverlib::IntUnregister(driverlib::INT_AUX_SWEV1);
    }

    /** \brief Waits until the task control interface is ready/idle
     *
     * This indicates that the task control interface is ready for the first request or that the last
     * request has been completed. If a timeout mechanisms is not available, the implementation may be
     * simplified.
     *
     * \note For the OSAL "None" implementation, a non-zero timeout corresponds to infinite timeout.
     *
     * \param[in]      timeoutUs
     *     Minimum timeout, in microseconds
     *
     * \return
     *     Whether the task control interface is now idle/ready
     */
    unsafe fn osal_wait_on_ctrl_ready(&self, timeout_us: u32) -> bool {
        if timeout_us > 0 {
            // while (!(HWREG(AUX_EVCTL_BASE + AUX_EVCTL_O_EVTOAONFLAGS) & AUX_EVCTL_EVTOAONFLAGS_SWEV0_M));
            while self.aux_evctl.evtoaonflags.read().swev0().bit_is_clear() {}

            true
        } else {
            // return (HWREG(AUX_EVCTL_BASE + AUX_EVCTL_O_EVTOAONFLAGS) & AUX_EVCTL_EVTOAONFLAGS_SWEV0_M);
            self.aux_evctl.evtoaonflags.read().swev0().bit_is_set()
        }
    } // osalWaitOnCtrlReady

    /** \brief OSAL "None": Enables the AUX domain and Sensor Controller for access from the MCU domain
     *
     * This function must be called before accessing/using any of the following:
     * - Oscillator control registers
     * - AUX ADI registers
     * - AUX module registers and AUX RAM
     * - SCIF API functions, except \ref scifOsalEnableAuxDomainAccess()
     * - SCIF data structures
     *
     * The application is responsible for:
     * - Registering the last set access control state
     * - Ensuring that this control is thread-safe
     */
    unsafe fn scif_osal_enable_aux_domain_access(&self) {
        // Force on AUX domain clock and bus connection
        // HWREG(driverlib::AON_WUC_BASE + driverlib::AON_WUC_O_AUXCTL) |= driverlib::AON_WUC_AUXCTL_AUX_FORCE_ON_M;
        self.aon_wuc
            .auxctl
            .modify(|_r, w| w.aux_force_on().set_bit());

        // HWREG(driverlib::AON_RTC_BASE + driverlib::AON_RTC_O_SYNC);
        self.aon_rtc.sync.read();

        // Wait for it to take effect
        // while (!(HWREG(driverlib::AON_WUC_BASE + driverlib::AON_WUC_O_PWRSTAT) & driverlib::AON_WUC_PWRSTAT_AUX_PD_ON_M));
        while self.aon_wuc.pwrstat.read().aux_pd_on().bit_is_clear() {}
    } // scifOsalEnableAuxDomainAccess

    /** \brief OSAL "None": Disables the AUX domain and Sensor Controller for access from the MCU domain
     *
     * The application is responsible for:
     * - Registering the last set access control state
     * - Ensuring that this control is thread-safe
     */
    unsafe fn scif_osal_disable_aux_domain_access(&self) {
        // Force on AUX domain bus connection
        // HWREG(AON_WUC_BASE + AON_WUC_O_AUXCTL) &= ~AON_WUC_AUXCTL_AUX_FORCE_ON_M;
        self.aon_wuc
            .auxctl
            .modify(|_r, w| w.aux_force_on().clear_bit());

        // HWREG(AON_RTC_BASE + AON_RTC_O_SYNC);
        self.aon_rtc.sync.read();

        // Wait for it to take effect
        // while (HWREG(AON_WUC_BASE + AON_WUC_O_PWRSTAT) & AON_WUC_PWRSTAT_AUX_PD_ON_M);
        while self.aon_wuc.pwrstat.read().aux_pd_on().bit_is_set() {}
    } // scifOsalDisableAuxDomainAccess
}
