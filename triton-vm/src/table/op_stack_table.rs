use std::cmp::Ordering;

use itertools::Itertools;
use ndarray::s;
use ndarray::ArrayViewMut2;
use num_traits::One;
use strum::EnumCount;
use strum_macros::Display;
use strum_macros::EnumCount as EnumCountMacro;
use strum_macros::EnumIter;
use twenty_first::shared_math::b_field_element::BFieldElement;
use twenty_first::shared_math::traits::Inverse;
use twenty_first::shared_math::x_field_element::XFieldElement;

use OpStackTableChallengeId::*;

use crate::cross_table_arguments::CrossTableArg;
use crate::cross_table_arguments::PermArg;
use crate::op_stack::OP_STACK_REG_COUNT;
use crate::table::base_matrix::AlgebraicExecutionTrace;
use crate::table::base_table::Extendable;
use crate::table::base_table::InheritsFromTable;
use crate::table::base_table::Table;
use crate::table::base_table::TableLike;
use crate::table::challenges::TableChallenges;
use crate::table::constraint_circuit::ConstraintCircuit;
use crate::table::constraint_circuit::ConstraintCircuitBuilder;
use crate::table::constraint_circuit::DualRowIndicator;
use crate::table::constraint_circuit::DualRowIndicator::*;
use crate::table::constraint_circuit::SingleRowIndicator;
use crate::table::constraint_circuit::SingleRowIndicator::Row;
use crate::table::extension_table::ExtensionTable;
use crate::table::extension_table::QuotientableExtensionTable;
use crate::table::table_column::OpStackBaseTableColumn;
use crate::table::table_column::OpStackBaseTableColumn::*;
use crate::table::table_column::OpStackExtTableColumn;
use crate::table::table_column::OpStackExtTableColumn::*;
use crate::table::table_column::ProcessorBaseTableColumn;

pub const OP_STACK_TABLE_NUM_PERMUTATION_ARGUMENTS: usize = 1;
pub const OP_STACK_TABLE_NUM_EVALUATION_ARGUMENTS: usize = 0;
pub const OP_STACK_TABLE_NUM_EXTENSION_CHALLENGES: usize = OpStackTableChallengeId::COUNT;

pub const BASE_WIDTH: usize = OpStackBaseTableColumn::COUNT;
pub const EXT_WIDTH: usize = OpStackExtTableColumn::COUNT;
pub const FULL_WIDTH: usize = BASE_WIDTH + EXT_WIDTH;

#[derive(Debug, Clone)]
pub struct OpStackTable {
    inherited_table: Table<BFieldElement>,
}

impl InheritsFromTable<BFieldElement> for OpStackTable {
    fn inherited_table(&self) -> &Table<BFieldElement> {
        &self.inherited_table
    }

    fn mut_inherited_table(&mut self) -> &mut Table<BFieldElement> {
        &mut self.inherited_table
    }
}

#[derive(Debug, Clone)]
pub struct ExtOpStackTable {
    pub(crate) inherited_table: Table<XFieldElement>,
}

impl Default for ExtOpStackTable {
    fn default() -> Self {
        Self {
            inherited_table: Table::new(
                BASE_WIDTH,
                FULL_WIDTH,
                vec![],
                "EmptyExtOpStackTable".to_string(),
            ),
        }
    }
}

impl QuotientableExtensionTable for ExtOpStackTable {}

impl InheritsFromTable<XFieldElement> for ExtOpStackTable {
    fn inherited_table(&self) -> &Table<XFieldElement> {
        &self.inherited_table
    }

    fn mut_inherited_table(&mut self) -> &mut Table<XFieldElement> {
        &mut self.inherited_table
    }
}

impl TableLike<BFieldElement> for OpStackTable {}

impl Extendable for OpStackTable {
    fn get_padding_rows(&self) -> (Option<usize>, Vec<Vec<BFieldElement>>) {
        panic!("This function should not be called: the Op Stack Table implements `.pad` directly.")
    }

    fn pad(&mut self, padded_height: usize) {
        let max_clock = self.data().len() as u64 - 1;
        let num_padding_rows = padded_height - self.data().len();

        let template_index = self
            .data()
            .iter()
            .enumerate()
            .find(|(_, row)| row[usize::from(CLK)].value() == max_clock)
            .map(|(idx, _)| idx)
            .expect("Op Stack Table must contain row with clock cycle equal to max cycle.");
        let insertion_index = template_index + 1;

        let padding_template = &mut self.mut_data()[template_index];
        padding_template[usize::from(InverseOfClkDiffMinusOne)] = 0_u64.into();

        let mut padding_rows = vec![];
        while padding_rows.len() < num_padding_rows {
            let mut padding_row = padding_template.clone();
            padding_row[usize::from(CLK)] += (padding_rows.len() as u32 + 1).into();
            padding_rows.push(padding_row)
        }

        if let Some(row) = padding_rows.last_mut() {
            if let Some(next_row) = self.data().get(insertion_index) {
                let clk_diff = next_row[usize::from(CLK)] - row[usize::from(CLK)];
                row[usize::from(InverseOfClkDiffMinusOne)] =
                    (clk_diff - BFieldElement::one()).inverse_or_zero();
            }
        }

        let old_tail_length = self.data().len() - insertion_index;
        self.mut_data().append(&mut padding_rows);
        self.mut_data()[insertion_index..].rotate_left(old_tail_length);

        assert_eq!(padded_height, self.data().len());
    }
}

impl TableLike<XFieldElement> for ExtOpStackTable {}

impl ExtOpStackTable {
    pub fn ext_initial_constraints_as_circuits(
    ) -> Vec<ConstraintCircuit<OpStackTableChallenges, SingleRowIndicator<FULL_WIDTH>>> {
        let circuit_builder = ConstraintCircuitBuilder::new(FULL_WIDTH);
        let one = circuit_builder.b_constant(1_u32.into());

        let clk = circuit_builder.input(Row(CLK.into()));
        let ib1 = circuit_builder.input(Row(IB1ShrinkStack.into()));
        let osp = circuit_builder.input(Row(OSP.into()));
        let osv = circuit_builder.input(Row(OSV.into()));
        let rppa = circuit_builder.input(Row(RunningProductPermArg.into()));
        let rpcjd = circuit_builder.input(Row(AllClockJumpDifferencesPermArg.into()));

        let clk_is_0 = clk;
        let osv_is_0 = osv;
        let osp_is_16 = osp - circuit_builder.b_constant(16_u32.into());

        // The running product for the permutation argument `rppa` starts off having accumulated the
        // first row. Note that `clk` and `osv` are constrained to be 0, and `osp` to be 16.
        let compressed_row = circuit_builder.challenge(Ib1Weight) * ib1
            + circuit_builder.challenge(OspWeight) * circuit_builder.b_constant(16_u32.into());
        let processor_perm_indeterminate = circuit_builder.challenge(ProcessorPermIndeterminate);
        let rppa_initial = processor_perm_indeterminate - compressed_row;
        let rppa_starts_correctly = rppa - rppa_initial;

        // The running product for clock jump differences starts with
        // one
        let rpcjd_starts_correctly = rpcjd - one;

        [
            clk_is_0,
            osv_is_0,
            osp_is_16,
            rppa_starts_correctly,
            rpcjd_starts_correctly,
        ]
        .map(|circuit| circuit.consume())
        .to_vec()
    }

    pub fn ext_consistency_constraints_as_circuits(
    ) -> Vec<ConstraintCircuit<OpStackTableChallenges, SingleRowIndicator<FULL_WIDTH>>> {
        // no further constraints
        vec![]
    }

    pub fn ext_transition_constraints_as_circuits(
    ) -> Vec<ConstraintCircuit<OpStackTableChallenges, DualRowIndicator<FULL_WIDTH>>> {
        let circuit_builder = ConstraintCircuitBuilder::<
            OpStackTableChallenges,
            DualRowIndicator<FULL_WIDTH>,
        >::new(2 * FULL_WIDTH);
        let one = circuit_builder.b_constant(1u32.into());

        let clk = circuit_builder.input(CurrentRow(CLK.into()));
        let ib1_shrink_stack = circuit_builder.input(CurrentRow(IB1ShrinkStack.into()));
        let osp = circuit_builder.input(CurrentRow(OSP.into()));
        let osv = circuit_builder.input(CurrentRow(OSV.into()));
        let clk_di = circuit_builder.input(CurrentRow(InverseOfClkDiffMinusOne.into()));
        let rpcjd = circuit_builder.input(CurrentRow(AllClockJumpDifferencesPermArg.into()));
        let rppa = circuit_builder.input(CurrentRow(RunningProductPermArg.into()));

        let clk_next = circuit_builder.input(NextRow(CLK.into()));
        let ib1_shrink_stack_next = circuit_builder.input(NextRow(IB1ShrinkStack.into()));
        let osp_next = circuit_builder.input(NextRow(OSP.into()));
        let osv_next = circuit_builder.input(NextRow(OSV.into()));
        let rpcjd_next = circuit_builder.input(NextRow(AllClockJumpDifferencesPermArg.into()));
        let rppa_next = circuit_builder.input(NextRow(RunningProductPermArg.into()));

        // the osp increases by 1 or the osp does not change
        //
        // $(osp' - (osp + 1))·(osp' - osp) = 0$
        let osp_increases_by_1_or_does_not_change =
            (osp_next.clone() - (osp.clone() + one.clone())) * (osp_next.clone() - osp.clone());

        // the osp increases by 1 or the osv does not change OR the ci shrinks the OpStack
        //
        // $ (osp' - (osp + 1)) · (osv' - osv) · (1 - ib1) = 0$
        let osp_increases_by_1_or_osv_does_not_change_or_shrink_stack = (osp_next.clone()
            - (osp.clone() + one.clone()))
            * (osv_next.clone() - osv)
            * (one.clone() - ib1_shrink_stack);

        // The clock jump difference inverse is consistent
        // with the clock cycles.
        //     clk_di' = (clk' - clk - 1)^-1 unless osp changes
        // <=> (osp' - osp - 1) * (1 - clk_di' * (clk' - clk - 1)) * clk_di' = 0
        //  /\ (osp' - osp - 1) * (1 - clk_di' * (clk' - clk - 1)) * (clk' - clk - 1) = 0
        let osp_changes = osp_next.clone() - osp.clone() - one.clone();
        let clk_diff_minus_one = clk_next.clone() - clk.clone() - one.clone();
        let clkdi_is_cdmo_inverse = clk_di.clone() * clk_diff_minus_one.clone() - one.clone();
        let clk_di_is_zero_or_cdmo_inverse_or_osp_changes =
            osp_changes.clone() * clkdi_is_cdmo_inverse.clone() * clk_di.clone();
        let cdmo_is_zero_or_clkdi_inverse_or_osp_changes =
            osp_changes * clkdi_is_cdmo_inverse * clk_diff_minus_one;

        // The running product for clock jump differences `rpcjd`
        // accumulates a factor (beta - clk' + clk) if
        //  - the op stack pointer `osp` remains the same; and
        //  - the clock jump difference is 2 or greater.
        //
        //   (clk' - clk - 1) * (1 - osp' + osp) * (cjdrp' - cjdrp * (beta - clk' + clk))
        // + (1 - (clk' - clk - 1) * clk_di) * (cjdrp' - cjdrp)
        // + (osp' - osp) * (cjdrp' - cjdrp)
        let beta = circuit_builder.challenge(AllClockJumpDifferencesMultiPermIndeterminate);
        let cjdrp_updates_correctly = (clk_next.clone() - clk.clone() - one.clone())
            * (one.clone() - osp_next.clone() + osp.clone())
            * (rpcjd_next.clone() - rpcjd.clone() * (beta - clk_next.clone() + clk.clone()))
            + (one.clone() - (clk_next.clone() - clk - one) * clk_di)
                * (rpcjd_next.clone() - rpcjd.clone())
            + (osp_next.clone() - osp) * (rpcjd_next - rpcjd);

        // The running product for the permutation argument `rppa` is updated correctly.
        let alpha = circuit_builder.challenge(ProcessorPermIndeterminate);
        let compressed_row = circuit_builder.challenge(ClkWeight) * clk_next
            + circuit_builder.challenge(Ib1Weight) * ib1_shrink_stack_next
            + circuit_builder.challenge(OspWeight) * osp_next
            + circuit_builder.challenge(OsvWeight) * osv_next;

        let rppa_updates_correctly = rppa_next - rppa * (alpha - compressed_row);

        [
            osp_increases_by_1_or_does_not_change,
            osp_increases_by_1_or_osv_does_not_change_or_shrink_stack,
            clk_di_is_zero_or_cdmo_inverse_or_osp_changes,
            cdmo_is_zero_or_clkdi_inverse_or_osp_changes,
            cjdrp_updates_correctly,
            rppa_updates_correctly,
        ]
        .map(|circuit| circuit.consume())
        .to_vec()
    }

    pub fn ext_terminal_constraints_as_circuits(
    ) -> Vec<ConstraintCircuit<OpStackTableChallenges, SingleRowIndicator<FULL_WIDTH>>> {
        // no further constraints
        vec![]
    }
}

impl OpStackTable {
    pub fn new(inherited_table: Table<BFieldElement>) -> Self {
        Self { inherited_table }
    }

    pub fn new_prover(matrix: Vec<Vec<BFieldElement>>) -> Self {
        let inherited_table =
            Table::new(BASE_WIDTH, FULL_WIDTH, matrix, "OpStackTable".to_string());
        Self { inherited_table }
    }

    pub fn fill_trace(
        op_stack_table: &mut ArrayViewMut2<BFieldElement>,
        aet: &AlgebraicExecutionTrace,
    ) {
        // Store the registers relevant for the Op Stack Table, i.e., CLK, IB1, OSP, and OSV,
        // with OSP as the key. Preserves, thus allows reusing, the order of the processor's
        // rows, which are sorted by CLK.
        let mut pre_processed_op_stack_table: Vec<Vec<_>> = vec![];
        for processor_row in aet.processor_matrix.iter() {
            let clk = processor_row[usize::from(ProcessorBaseTableColumn::CLK)];
            let ib1 = processor_row[usize::from(ProcessorBaseTableColumn::IB1)];
            let osp = processor_row[usize::from(ProcessorBaseTableColumn::OSP)];
            let osv = processor_row[usize::from(ProcessorBaseTableColumn::OSV)];
            // The (honest) prover can only grow the Op Stack's size by at most 1 per execution
            // step. Hence, the following (a) works, and (b) sorts.
            let osp_minus_16 = osp.value() as usize - OP_STACK_REG_COUNT;
            let op_stack_row = (clk, ib1, osv);
            match osp_minus_16.cmp(&pre_processed_op_stack_table.len()) {
                Ordering::Less => pre_processed_op_stack_table[osp_minus_16].push(op_stack_row),
                Ordering::Equal => pre_processed_op_stack_table.push(vec![op_stack_row]),
                Ordering::Greater => panic!("OSP must increase by at most 1 per execution step."),
            }
        }

        // Move the rows into the Op Stack Table, sorted by OSP first, CLK second.
        let mut op_stack_table_row = 0;
        for (osp_minus_16, rows_with_this_osp) in
            pre_processed_op_stack_table.into_iter().enumerate()
        {
            let osp = BFieldElement::new((osp_minus_16 + OP_STACK_REG_COUNT) as u64);
            for (clk, ib1, osv) in rows_with_this_osp {
                op_stack_table[(op_stack_table_row, usize::from(CLK))] = clk;
                op_stack_table[(op_stack_table_row, usize::from(IB1ShrinkStack))] = ib1;
                op_stack_table[(op_stack_table_row, usize::from(OSP))] = osp;
                op_stack_table[(op_stack_table_row, usize::from(OSV))] = osv;
                op_stack_table_row += 1;
            }
        }
        assert_eq!(aet.processor_matrix.len(), op_stack_table_row);

        // Set inverse of (clock difference - 1).
        // The Op Stack Table and the Processor Table have the same length.
        for row_idx in 0..aet.processor_matrix.len() - 1 {
            let (mut curr_row, next_row) =
                op_stack_table.multi_slice_mut((s![row_idx, ..], s![row_idx + 1, ..]));
            let clk_diff = next_row[usize::from(CLK)] - curr_row[usize::from(CLK)];
            let clk_diff_minus_1 = clk_diff - BFieldElement::one();
            let clk_diff_minus_1_inverse = clk_diff_minus_1.inverse_or_zero();
            curr_row[usize::from(InverseOfClkDiffMinusOne)] = clk_diff_minus_1_inverse;
        }
    }

    pub fn extend(&self, challenges: &OpStackTableChallenges) -> ExtOpStackTable {
        let mut extension_matrix: Vec<Vec<XFieldElement>> = Vec::with_capacity(self.data().len());
        let mut running_product = PermArg::default_initial();
        let mut all_clock_jump_differences_running_product = PermArg::default_initial();

        let mut previous_row: Option<Vec<BFieldElement>> = None;
        for row in self.data().iter() {
            let mut extension_row = [0.into(); FULL_WIDTH];
            extension_row[..BASE_WIDTH]
                .copy_from_slice(&row.iter().map(|elem| elem.lift()).collect_vec());

            let clk = extension_row[usize::from(CLK)];
            let ib1 = extension_row[usize::from(IB1ShrinkStack)];
            let osp = extension_row[usize::from(OSP)];
            let osv = extension_row[usize::from(OSV)];

            let clk_w = challenges.clk_weight;
            let ib1_w = challenges.ib1_weight;
            let osp_w = challenges.osp_weight;
            let osv_w = challenges.osv_weight;

            // compress multiple values within one row so they become one value
            let compressed_row_for_permutation_argument =
                clk * clk_w + ib1 * ib1_w + osp * osp_w + osv * osv_w;

            // compute the running *product* of the compressed column (for permutation argument)
            running_product *=
                challenges.processor_perm_indeterminate - compressed_row_for_permutation_argument;
            extension_row[usize::from(RunningProductPermArg)] = running_product;

            // clock jump difference
            if let Some(prow) = previous_row {
                if prow[usize::from(OSP)] == row[usize::from(OSP)] {
                    let clock_jump_difference =
                        (row[usize::from(CLK)] - prow[usize::from(CLK)]).lift();
                    if clock_jump_difference != XFieldElement::one() {
                        all_clock_jump_differences_running_product *= challenges
                            .all_clock_jump_differences_multi_perm_indeterminate
                            - clock_jump_difference;
                    }
                }
            }
            extension_row[usize::from(AllClockJumpDifferencesPermArg)] =
                all_clock_jump_differences_running_product;

            previous_row = Some(row.clone());
            extension_matrix.push(extension_row.to_vec());
        }

        assert_eq!(self.data().len(), extension_matrix.len());
        let inherited_table = self.new_from_lifted_matrix(extension_matrix);
        ExtOpStackTable { inherited_table }
    }

    pub fn for_verifier() -> ExtOpStackTable {
        let inherited_table = Table::new(
            BASE_WIDTH,
            FULL_WIDTH,
            vec![],
            "ExtOpStackTable".to_string(),
        );
        let base_table = Self { inherited_table };
        let empty_matrix: Vec<Vec<XFieldElement>> = vec![];
        let extension_table = base_table.new_from_lifted_matrix(empty_matrix);

        ExtOpStackTable {
            inherited_table: extension_table,
        }
    }
}

impl ExtOpStackTable {
    pub fn new(inherited_table: Table<XFieldElement>) -> Self {
        Self { inherited_table }
    }
}

#[derive(Debug, Copy, Clone, Display, EnumCountMacro, EnumIter, PartialEq, Eq, Hash)]
pub enum OpStackTableChallengeId {
    ProcessorPermIndeterminate,
    ClkWeight,
    Ib1Weight,
    OsvWeight,
    OspWeight,
    AllClockJumpDifferencesMultiPermIndeterminate,
}

impl From<OpStackTableChallengeId> for usize {
    fn from(val: OpStackTableChallengeId) -> Self {
        val as usize
    }
}

impl TableChallenges for OpStackTableChallenges {
    type Id = OpStackTableChallengeId;

    #[inline]
    fn get_challenge(&self, id: Self::Id) -> XFieldElement {
        match id {
            ProcessorPermIndeterminate => self.processor_perm_indeterminate,
            ClkWeight => self.clk_weight,
            Ib1Weight => self.ib1_weight,
            OsvWeight => self.osv_weight,
            OspWeight => self.osp_weight,
            AllClockJumpDifferencesMultiPermIndeterminate => {
                self.all_clock_jump_differences_multi_perm_indeterminate
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpStackTableChallenges {
    /// The weight that combines two consecutive rows in the
    /// permutation/evaluation column of the op-stack table.
    pub processor_perm_indeterminate: XFieldElement,

    /// Weights for condensing part of a row into a single column. (Related to processor table.)
    pub clk_weight: XFieldElement,
    pub ib1_weight: XFieldElement,
    pub osv_weight: XFieldElement,
    pub osp_weight: XFieldElement,

    /// Weight for accumulating all clock jump differences
    pub all_clock_jump_differences_multi_perm_indeterminate: XFieldElement,
}

impl ExtensionTable for ExtOpStackTable {}
