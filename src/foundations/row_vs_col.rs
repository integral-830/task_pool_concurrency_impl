use std::io::Seek;

pub trait ColumnTable {
    fn new() -> Self
    where
        Self: Sized;
    fn insert(&mut self, row: [i64; 10]);
    fn get(&self, row: usize, column: usize) -> i64;
    fn get_col_sum(&mut self, column: usize) -> i64;
}

pub struct RowStore {
    data: Vec<[i64; 10]>,
}

pub struct ColStore {
    data: [Vec<i64>; 10],
}

impl ColumnTable for RowStore {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self { data: Vec::new() }
    }

    fn insert(&mut self, row: [i64; 10]) {
        self.data.push(row);
    }

    fn get(&self, row: usize, column: usize) -> i64 {
        self.data[row][column]
    }

    fn get_col_sum(&mut self, column: usize) -> i64 {
        self.data.iter().map(|row| row[column]).sum()
    }
}

impl ColumnTable for ColStore {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            data: std::array::from_fn(|_| Vec::new()),
        }
    }

    fn insert(&mut self, row: [i64; 10]) {
        for (col, val) in row.into_iter().enumerate() {
            self.data[col].push(val);
        }
    }

    fn get(&self, row: usize, column: usize) -> i64 {
        self.data[column][row]
    }

    fn get_col_sum(&mut self, column: usize) -> i64 {
        self.data[column].iter().sum()
    }
}

pub struct PaxPage<const PAGE_ROWS: usize> {
    columns: [Vec<i64>; 10],
    rows: usize,
}

pub struct PaxStore<const PAGE_ROWS: usize> {
    pages: Vec<PaxPage<PAGE_ROWS>>,
}

impl<const PAGE_ROWS: usize> ColumnTable for PaxStore<PAGE_ROWS> {
    fn new() -> Self {
        Self { pages: Vec::new() }
    }

    fn insert(&mut self, row: [i64; 10]) {
        if self
            .pages
            .last()
            .map_or(true, |page| page.rows == PAGE_ROWS)
        {
            self.pages.push(PaxPage {
                columns: std::array::from_fn(|_| Vec::with_capacity(PAGE_ROWS)),
                rows: 0,
            });
        }

        let page = self.pages.last_mut().unwrap();

        for (column, value) in row.into_iter().enumerate() {
            page.columns[column].push(value);
        }

        page.rows += 1;
    }

    fn get(&self, row: usize, column: usize) -> i64 {
        let page_index = row / PAGE_ROWS;
        let row_in_page = row % PAGE_ROWS;

        self.pages[page_index].columns[column][row_in_page]
    }

    fn get_col_sum(&mut self, column: usize) -> i64 {
        let mut sum = 0;

        for page in &self.pages {
            for value in &page.columns[column] {
                sum += *value;
            }
        }

        sum
    }
}
